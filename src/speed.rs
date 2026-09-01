use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use reqwest::{Body, Client, Response, Url};
use serde::Deserialize;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinSet;

const FAST_HOME: &str = "https://fast.com/";
const FAST_API: &str = "https://api.fast.com/netflix/speedtest/v2";
const MAX_METADATA_BYTES: usize = 2 * 1024 * 1024;
const MAX_TARGETS: usize = 16;
const TARGET_REQUEST_COUNT: usize = 5;
const MAX_CONCURRENT_REQUESTS: usize = 4;
const UPLOAD_BYTES: usize = 8 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const SPEED_UPDATE_INTERVAL: Duration = Duration::from_millis(100);

/// The stage currently being performed by the speed test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestPhase {
    Idle,
    FetchingTargets,
    Download,
    Upload,
    Latency,
    Complete,
    Failed,
}

/// An incremental update emitted while a speed test is running.
#[derive(Debug, Clone)]
pub enum SpeedUpdate {
    Phase(TestPhase),
    Progress(f64),
    DownloadMbps(f64),
    UploadMbps(f64),
    LatencyMs(f64),
    Complete(TestResult),
    Failed(String),
}

/// The final measured values. Throughput is decimal megabits per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TestResult {
    pub download_mbps: f64,
    pub upload_mbps: f64,
    pub latency_ms: f64,
}

#[derive(Debug, Deserialize)]
struct TargetsResponse {
    #[serde(default)]
    targets: Vec<RawTarget>,
}

#[derive(Debug, Deserialize)]
struct RawTarget {
    // An individual malformed target should not make otherwise usable targets
    // unusable. If every target is malformed, discovery fails below.
    url: Option<String>,
}

/// Run a complete Fast.com test and report every stage through `updates`.
pub async fn run_speed_test(updates: UnboundedSender<SpeedUpdate>) -> Result<TestResult> {
    let result = run_speed_test_inner(&updates).await;
    match result {
        Ok(result) => {
            let _ = updates.send(SpeedUpdate::Phase(TestPhase::Complete));
            let _ = updates.send(SpeedUpdate::Progress(1.0));
            let _ = updates.send(SpeedUpdate::Complete(result));
            Ok(result)
        }
        Err(error) => {
            let message = format_error(&error);
            let _ = updates.send(SpeedUpdate::Phase(TestPhase::Failed));
            let _ = updates.send(SpeedUpdate::Failed(message));
            Err(error)
        }
    }
}

async fn run_speed_test_inner(updates: &UnboundedSender<SpeedUpdate>) -> Result<TestResult> {
    send_phase(updates, TestPhase::FetchingTargets, 0.0);

    let client = Client::builder()
        .user_agent("speedtest-ratatui/1.0")
        .timeout(REQUEST_TIMEOUT)
        .build()
        .context("building HTTP client")?;
    let token = fetch_token(&client)
        .await
        .context("fetching Fast.com token")?;
    let targets = fetch_targets(&client, &token)
        .await
        .context("fetching Fast.com measurement targets")?;
    send_progress(updates, 0.08);

    send_phase(updates, TestPhase::Latency, 0.1);
    let latency_ms = run_latency_probes(&client, &targets, updates)
        .await
        .context("measuring latency")?;
    let _ = updates.send(SpeedUpdate::LatencyMs(latency_ms));
    send_progress(updates, 0.18);

    send_phase(updates, TestPhase::Download, 0.2);
    let download_mbps = run_downloads(&client, &targets, updates)
        .await
        .context("measuring download throughput")?;
    let _ = updates.send(SpeedUpdate::DownloadMbps(download_mbps));
    send_progress(updates, 0.65);

    send_phase(updates, TestPhase::Upload, 0.68);
    let upload_mbps = run_uploads(&client, &targets, updates)
        .await
        .context("measuring upload throughput")?;
    let _ = updates.send(SpeedUpdate::UploadMbps(upload_mbps));
    send_progress(updates, 0.98);

    Ok(TestResult {
        download_mbps,
        upload_mbps,
        latency_ms,
    })
}

fn send_phase(updates: &UnboundedSender<SpeedUpdate>, phase: TestPhase, progress: f64) {
    let _ = updates.send(SpeedUpdate::Phase(phase));
    send_progress(updates, progress);
}

fn send_progress(updates: &UnboundedSender<SpeedUpdate>, progress: f64) {
    let _ = updates.send(SpeedUpdate::Progress(progress.clamp(0.0, 1.0)));
}

async fn fetch_token(client: &Client) -> Result<String> {
    let homepage = Url::parse(FAST_HOME).context("invalid Fast.com homepage URL")?;
    let response = client
        .get(homepage.clone())
        .send()
        .await
        .context("requesting Fast.com homepage")?;
    ensure_success(&response, "Fast.com homepage")?;
    let html = String::from_utf8(
        read_limited(response, MAX_METADATA_BYTES, "Fast.com homepage")
            .await
            .context("reading Fast.com homepage")?,
    )
    .context("decoding Fast.com homepage as UTF-8")?;

    // Some deployments include the token in the document itself. Prefer it,
    // then follow the app bundle used by the regular Fast.com application.
    if let Some(token) = extract_token(html.as_str()) {
        return Ok(token);
    }
    let script_path = extract_app_script_path(html.as_str())
        .ok_or_else(|| anyhow!("Fast.com homepage contains no application bundle"))?;
    let script_url = homepage
        .join(script_path)
        .context("resolving Fast.com application bundle URL")?;
    let response = client
        .get(script_url)
        .send()
        .await
        .context("requesting Fast.com application bundle")?;
    ensure_success(&response, "Fast.com application bundle")?;
    let script = String::from_utf8(
        read_limited(response, MAX_METADATA_BYTES, "Fast.com application bundle")
            .await
            .context("reading Fast.com application bundle")?,
    )
    .context("decoding Fast.com application bundle as UTF-8")?;
    extract_token(script.as_str())
        .ok_or_else(|| anyhow!("Fast.com application bundle contains no API token"))
}

async fn fetch_targets(client: &Client, token: &str) -> Result<Vec<Url>> {
    if token.is_empty() {
        bail!("Fast.com returned an empty API token");
    }

    let mut endpoint = Url::parse(FAST_API).context("invalid Fast.com API URL")?;
    {
        let target_count = TARGET_REQUEST_COUNT.to_string();
        let mut query = endpoint.query_pairs_mut();
        query.append_pair("https", "true");
        query.append_pair("token", token);
        query.append_pair("urlCount", &target_count);
    }

    let response = client
        .get(endpoint)
        .send()
        .await
        .context("requesting Fast.com targets")?;
    ensure_success(&response, "Fast.com targets")?;
    let body = read_limited(response, MAX_METADATA_BYTES, "Fast.com targets")
        .await
        .context("reading Fast.com targets")?;
    let parsed: TargetsResponse =
        serde_json::from_slice(&body).context("parsing Fast.com targets JSON")?;

    let mut targets = Vec::with_capacity(parsed.targets.len().min(MAX_TARGETS));
    for target in parsed.targets.into_iter().take(MAX_TARGETS) {
        let Some(raw_url) = target.url else {
            continue;
        };
        let Ok(url) = Url::parse(raw_url.trim()) else {
            continue;
        };
        if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() {
            targets.push(url);
        }
    }
    if targets.is_empty() {
        bail!("Fast.com returned no usable measurement targets");
    }
    Ok(targets)
}

async fn run_latency_probes(
    client: &Client,
    targets: &[Url],
    updates: &UnboundedSender<SpeedUpdate>,
) -> Result<f64> {
    const PROBE_COUNT: usize = 8;
    let mut samples = Vec::with_capacity(PROBE_COUNT);
    let mut first_error = None;

    for i in 0..PROBE_COUNT {
        let target = &targets[i % targets.len()];
        match measure_latency_single(client, target).await {
            Ok(lat) => {
                let _ = updates.send(SpeedUpdate::LatencyMs(lat));
                samples.push(lat);
                send_progress(updates, 0.10 + 0.08 * ((i + 1) as f64 / PROBE_COUNT as f64));
            }
            Err(e) => {
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }
        if i + 1 < PROBE_COUNT {
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    }

    if samples.is_empty() {
        return Err(first_error.unwrap_or_else(|| anyhow!("all latency probes failed")));
    }

    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = samples[samples.len() / 2];
    Ok(median)
}

async fn measure_latency_single(client: &Client, target: &Url) -> Result<f64> {
    let started = Instant::now();
    let response = client
        .get(target.clone())
        .header(reqwest::header::RANGE, "bytes=0-0")
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await
        .context("latency request failed")?;
    ensure_success(&response, "Fast.com latency request")?;
    let mut response = response;
    let _ = response.chunk().await.context("reading latency response")?;
    Ok(started.elapsed().as_secs_f64() * 1_000.0)
}

async fn run_downloads(
    client: &Client,
    targets: &[Url],
    updates: &UnboundedSender<SpeedUpdate>,
) -> Result<f64> {
    let started = Instant::now();
    let bytes_seen = Arc::new(AtomicU64::new(0));
    let mut jobs = JoinSet::new();
    let mut next_target = 0usize;
    let mut active = 0usize;
    let mut completed = 0usize;
    let mut successful = 0usize;
    let mut first_error = None;

    while next_target < targets.len() || active > 0 {
        while next_target < targets.len() && active < MAX_CONCURRENT_REQUESTS {
            let target = targets[next_target].clone();
            next_target += 1;
            active += 1;
            let request_client = client.clone();
            let request_bytes = Arc::clone(&bytes_seen);
            let request_updates = updates.clone();
            jobs.spawn(async move {
                download_one(
                    &request_client,
                    &target,
                    &request_bytes,
                    &request_updates,
                    started,
                )
                .await
            });
        }

        let joined = jobs
            .join_next()
            .await
            .ok_or_else(|| anyhow!("download workers exited unexpectedly"))?;
        active -= 1;
        completed += 1;
        send_progress(
            updates,
            0.2 + 0.45 * (completed as f64 / targets.len() as f64),
        );
        match joined {
            Ok(Ok(_)) => successful += 1,
            Ok(Err(error)) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(anyhow!("download worker failed: {error}"));
                }
            }
        }
    }

    if successful == 0 {
        return Err(first_error.unwrap_or_else(|| anyhow!("all download targets failed")));
    }
    let elapsed = started.elapsed();
    let total_bytes = bytes_seen.load(Ordering::Relaxed);
    if total_bytes == 0 {
        bail!("download targets returned no body data");
    }
    Ok(mbps(total_bytes, elapsed))
}

async fn download_one(
    client: &Client,
    target: &Url,
    bytes_seen: &AtomicU64,
    updates: &UnboundedSender<SpeedUpdate>,
    started: Instant,
) -> Result<u64> {
    let response = client
        .get(target.clone())
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await
        .with_context(|| format!("GET {target}"))?;
    ensure_success(&response, "Fast.com download target")?;

    let mut response = response;
    let mut local_bytes = 0u64;
    let mut last_update = Instant::now();
    while let Some(chunk) = response.chunk().await.context("reading download stream")? {
        let chunk_len = chunk.len() as u64;
        local_bytes = local_bytes.saturating_add(chunk_len);
        let total = bytes_seen.fetch_add(chunk_len, Ordering::Relaxed) + chunk_len;
        if last_update.elapsed() >= SPEED_UPDATE_INTERVAL {
            let _ = updates.send(SpeedUpdate::DownloadMbps(mbps(total, started.elapsed())));
            last_update = Instant::now();
        }
    }
    if local_bytes == 0 {
        bail!("download target returned an empty body");
    }
    Ok(local_bytes)
}

async fn run_uploads(
    client: &Client,
    targets: &[Url],
    updates: &UnboundedSender<SpeedUpdate>,
) -> Result<f64> {
    let started = Instant::now();
    let sent_bytes = Arc::new(AtomicU64::new(0));
    let mut jobs = JoinSet::new();
    let mut next_target = 0usize;
    let mut active = 0usize;
    let mut completed = 0usize;
    let mut successful = 0usize;
    let mut first_error = None;

    while next_target < targets.len() || active > 0 {
        while next_target < targets.len() && active < MAX_CONCURRENT_REQUESTS {
            let target = targets[next_target].clone();
            next_target += 1;
            active += 1;
            let request_client = client.clone();
            let request_bytes = Arc::clone(&sent_bytes);
            let request_updates = updates.clone();
            jobs.spawn(async move {
                upload_one(
                    &request_client,
                    &target,
                    request_bytes,
                    request_updates,
                    started,
                )
                .await
            });
        }
        let joined = jobs
            .join_next()
            .await
            .ok_or_else(|| anyhow!("upload workers exited unexpectedly"))?;
        active -= 1;
        completed += 1;
        send_progress(
            updates,
            0.68 + 0.30 * (completed as f64 / targets.len() as f64),
        );
        match joined {
            Ok(Ok(_)) => {
                successful += 1;
                let total = sent_bytes.load(Ordering::Relaxed);
                let _ = updates.send(SpeedUpdate::UploadMbps(mbps(total, started.elapsed())));
            }
            Ok(Err(error)) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(anyhow!("upload worker failed: {error}"));
                }
            }
        }
    }

    if successful == 0 {
        let detail = first_error
            .map(|error| format!(": {error}"))
            .unwrap_or_default();
        bail!("all Fast.com upload targets failed{detail}");
    }
    let total_bytes = sent_bytes.load(Ordering::Relaxed);
    if total_bytes == 0 {
        bail!("Fast.com upload targets accepted no data");
    }
    Ok(mbps(total_bytes, started.elapsed()))
}

async fn upload_one(
    client: &Client,
    target: &Url,
    bytes_sent: Arc<AtomicU64>,
    updates: UnboundedSender<SpeedUpdate>,
    started: Instant,
) -> Result<u64> {
    const CHUNK_SIZE: usize = 64 * 1024;
    let chunk_data = vec![0u8; CHUNK_SIZE];
    let total_chunks = UPLOAD_BYTES.div_ceil(CHUNK_SIZE);
    let local_bytes = Arc::new(AtomicU64::new(0));
    let local_bytes_clone = Arc::clone(&local_bytes);
    let last_update = Arc::new(tokio::sync::Mutex::new(Instant::now()));

    let stream = futures_util::stream::unfold(0usize, move |state| {
        let chunk = chunk_data.clone();
        let w_bytes = Arc::clone(&local_bytes_clone);
        let g_bytes = Arc::clone(&bytes_sent);
        let up = updates.clone();
        let l_up = Arc::clone(&last_update);
        async move {
            if state < total_chunks {
                let remaining = UPLOAD_BYTES.saturating_sub(state * CHUNK_SIZE);
                let this_len = remaining.min(CHUNK_SIZE);
                let slice = if this_len == CHUNK_SIZE {
                    chunk
                } else {
                    chunk[..this_len].to_vec()
                };
                w_bytes.fetch_add(this_len as u64, Ordering::Relaxed);
                let total = g_bytes.fetch_add(this_len as u64, Ordering::Relaxed) + this_len as u64;
                let mut last = l_up.lock().await;
                if last.elapsed() >= SPEED_UPDATE_INTERVAL {
                    let _ = up.send(SpeedUpdate::UploadMbps(mbps(total, started.elapsed())));
                    *last = Instant::now();
                }
                Some((Ok::<_, std::io::Error>(slice), state + 1))
            } else {
                None
            }
        }
    });

    let body = Body::wrap_stream(stream);
    let response = client
        .post(target.clone())
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .header(reqwest::header::CONTENT_LENGTH, UPLOAD_BYTES.to_string())
        .body(body)
        .send()
        .await
        .with_context(|| format!("POST {target}"))?;
    ensure_success(&response, "Fast.com upload target")?;
    let uploaded = local_bytes.load(Ordering::Relaxed).max(UPLOAD_BYTES as u64);
    Ok(uploaded)
}

fn ensure_success(response: &Response, operation: &str) -> Result<()> {
    if response.status().is_success() {
        return Ok(());
    }
    let status = response.status();
    let reason = status.canonical_reason().unwrap_or("HTTP error");
    bail!("{operation} returned HTTP {status} ({reason})")
}

async fn read_limited(
    mut response: Response,
    max_bytes: usize,
    operation: &str,
) -> Result<Vec<u8>> {
    if let Some(length) = response.content_length() {
        if length > max_bytes as u64 {
            bail!("{operation} response exceeds {max_bytes} bytes");
        }
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len().saturating_add(chunk.len()) > max_bytes {
            bail!("{operation} response exceeds {max_bytes} bytes");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn mbps(bytes: u64, elapsed: Duration) -> f64 {
    let seconds = elapsed.as_secs_f64().max(f64::EPSILON);
    bytes as f64 * 8.0 / seconds / 1_000_000.0
}

fn extract_app_script_path(html: &str) -> Option<&str> {
    for marker in ["src=\"", "src='"] {
        let mut offset = 0usize;
        while let Some(found) = html[offset..].find(marker) {
            let start = offset + found + marker.len();
            let quote = marker.as_bytes()[marker.len() - 1] as char;
            let Some(end_rel) = html[start..].find(quote) else {
                break;
            };
            let end = start + end_rel;
            let candidate = &html[start..end];
            let without_query = candidate.split(['?', '#']).next().unwrap_or(candidate);
            if without_query.contains("app-") && without_query.ends_with(".js") {
                return Some(candidate);
            }
            offset = end + 1;
        }
    }
    None
}

fn extract_token(source: &str) -> Option<String> {
    let mut offset = 0usize;
    while let Some(found) = source[offset..].find("token") {
        let start = offset + found;
        let before = source[..start].chars().next_back();
        let after = source[start + "token".len()..].chars().next();
        if before.is_some_and(is_identifier_char) || after.is_some_and(is_identifier_char) {
            offset = start + "token".len();
            continue;
        }

        let mut cursor = start + "token".len();
        let key_quote = source.as_bytes().get(start.saturating_sub(1)).copied();
        if matches!(key_quote, Some(b'"') | Some(b'\''))
            && source.as_bytes().get(cursor).copied() == key_quote
        {
            cursor += 1;
        }
        while source
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            cursor += 1;
        }
        if source.as_bytes().get(cursor) != Some(&b':') {
            offset = start + "token".len();
            continue;
        }
        cursor += 1;
        while source
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            cursor += 1;
        }
        let quote = match source.as_bytes().get(cursor) {
            Some(b'\"') | Some(b'\'') => {
                let quote = source.as_bytes()[cursor];
                cursor += 1;
                Some(quote)
            }
            _ => None,
        };
        let token_start = cursor;
        while let Some(byte) = source.as_bytes().get(cursor) {
            if quote == Some(*byte)
                || (quote.is_none()
                    && (byte.is_ascii_whitespace() || *byte == b',' || *byte == b'}'))
            {
                break;
            }
            cursor += 1;
        }
        let token = &source[token_start..cursor];
        if valid_token(token) {
            return Some(token.to_owned());
        }
        offset = cursor.saturating_add(1);
    }
    None
}

fn is_identifier_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '$'
}

fn valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 512
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._~+/=-".contains(&byte))
}

fn format_error(error: &anyhow::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_token_from_javascript_and_json() {
        assert_eq!(
            extract_token(r#"return {urlCount: 5, token: "ABC123", https: true}"#),
            Some("ABC123".to_owned())
        );
        assert_eq!(
            extract_token(r#"{"token":"a_b-c+/="}"#),
            Some("a_b-c+/=".to_owned())
        );
    }

    #[test]
    fn rejects_token_like_identifier_and_invalid_value() {
        assert_eq!(extract_token(r#"mytoken: "bad" token: "ok!""#), None);
    }

    #[test]
    fn extracts_application_bundle_path() {
        let html =
            r#"<script src="/vendor.js"></script><script src="/app-abc123.js?x=1"></script>"#;
        assert_eq!(extract_app_script_path(html), Some("/app-abc123.js?x=1"));
    }

    #[test]
    fn calculates_decimal_megabits_from_bytes_and_elapsed_time() {
        assert!((mbps(1_000_000, Duration::from_secs(1)) - 8.0).abs() < f64::EPSILON);
        assert!((mbps(0, Duration::ZERO)).abs() < f64::EPSILON);
    }
}
