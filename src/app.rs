use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, Gauge, GraphType, Paragraph, Wrap},
    Frame,
};

use crate::speed::{SpeedUpdate, TestPhase, TestResult};

const BG: Color = Color::Rgb(9, 15, 25);
const PANEL: Color = Color::Rgb(15, 24, 38);
const PANEL_ALT: Color = Color::Rgb(22, 35, 54);
const PANEL_BORDER: Color = Color::Rgb(35, 55, 80);
const INK: Color = Color::Rgb(230, 240, 250);
const MUTED: Color = Color::Rgb(130, 155, 180);
const CYAN: Color = Color::Rgb(79, 216, 218);
const BLUE: Color = Color::Rgb(91, 145, 255);
const AMBER: Color = Color::Rgb(246, 185, 76);
const RED: Color = Color::Rgb(246, 104, 112);
const GREEN: Color = Color::Rgb(105, 218, 149);

/// State held by the terminal dashboard while a Fast.com measurement runs.
#[derive(Debug, Clone)]
pub struct App {
    pub phase: TestPhase,
    pub progress: f64,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub latency_ms: Option<f64>,
    pub status: String,
    pub error: Option<String>,
    pub running: bool,
    pub result: Option<TestResult>,
    pub download_points: Vec<(f64, f64)>,
    pub upload_points: Vec<(f64, f64)>,
    pub latency_points: Vec<(f64, f64)>,
    pub latency_samples: Vec<f64>,
    pub peak_download: Option<f64>,
    pub peak_upload: Option<f64>,
    pub min_latency: Option<f64>,
    pub max_latency: Option<f64>,
    pub avg_latency: Option<f64>,
    pub started_at: Option<std::time::Instant>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            phase: TestPhase::Idle,
            progress: 0.0,
            download_mbps: None,
            upload_mbps: None,
            latency_ms: None,
            status: ready_status().to_string(),
            error: None,
            running: false,
            result: None,
            download_points: Vec::new(),
            upload_points: Vec::new(),
            latency_points: Vec::new(),
            latency_samples: Vec::new(),
            peak_download: None,
            peak_upload: None,
            min_latency: None,
            max_latency: None,
            avg_latency: None,
            started_at: None,
        }
    }

    /// Fold one measurement event into the view model.
    pub fn apply_update(&mut self, update: SpeedUpdate) {
        match update {
            SpeedUpdate::Phase(phase) => {
                self.phase = phase;
                self.error = None;

                match phase {
                    TestPhase::Idle => {
                        self.running = false;
                        self.progress = 0.0;
                        self.status = ready_status().to_string();
                    }
                    TestPhase::FetchingTargets => {
                        // A target lookup marks the beginning of a fresh run. This
                        // also makes restart work when callers do not call reset first.
                        self.running = true;
                        self.progress = 0.0;
                        self.download_mbps = None;
                        self.upload_mbps = None;
                        self.latency_ms = None;
                        self.result = None;
                        self.download_points.clear();
                        self.upload_points.clear();
                        self.latency_points.clear();
                        self.latency_samples.clear();
                        self.peak_download = None;
                        self.peak_upload = None;
                        self.min_latency = None;
                        self.max_latency = None;
                        self.avg_latency = None;
                        self.started_at = Some(std::time::Instant::now());
                        self.status = phase_status(phase).to_string();
                    }
                    TestPhase::Download | TestPhase::Upload | TestPhase::Latency => {
                        self.running = true;
                        if self.started_at.is_none() {
                            self.started_at = Some(std::time::Instant::now());
                        }
                        self.status = phase_status(phase).to_string();
                    }
                    TestPhase::Complete => {
                        self.running = false;
                        self.progress = 1.0;
                        self.status = phase_status(phase).to_string();
                    }
                    TestPhase::Failed => {
                        self.running = false;
                        self.status = phase_status(phase).to_string();
                    }
                }
            }
            SpeedUpdate::Progress(progress) => {
                if progress.is_finite() {
                    self.progress = progress.clamp(0.0, 1.0);
                }
            }
            SpeedUpdate::DownloadMbps(value) => {
                if let Some(value) = valid_measurement(value) {
                    self.download_mbps = Some(value);
                    self.peak_download = Some(self.peak_download.map_or(value, |p| p.max(value)));
                    let t = self.started_at.map_or(0.0, |s| s.elapsed().as_secs_f64());
                    let t = if let Some(&(last_t, _)) = self.download_points.last() {
                        if t <= last_t {
                            last_t + 0.05
                        } else {
                            t
                        }
                    } else {
                        t
                    };
                    self.download_points.push((t, value));
                    if self.phase == TestPhase::Download {
                        self.status = "Measuring download throughput".to_string();
                    }
                }
            }
            SpeedUpdate::UploadMbps(value) => {
                if let Some(value) = valid_measurement(value) {
                    self.upload_mbps = Some(value);
                    self.peak_upload = Some(self.peak_upload.map_or(value, |p| p.max(value)));
                    let t = self.started_at.map_or(0.0, |s| s.elapsed().as_secs_f64());
                    let t = if let Some(&(last_t, _)) = self.upload_points.last() {
                        if t <= last_t {
                            last_t + 0.05
                        } else {
                            t
                        }
                    } else {
                        t
                    };
                    self.upload_points.push((t, value));
                    if self.phase == TestPhase::Upload {
                        self.status = "Measuring upload throughput".to_string();
                    }
                }
            }
            SpeedUpdate::LatencyMs(value) => {
                if let Some(value) = valid_measurement(value) {
                    self.latency_ms = Some(value);
                    self.latency_samples.push(value);
                    self.min_latency = Some(self.min_latency.map_or(value, |m| m.min(value)));
                    self.max_latency = Some(self.max_latency.map_or(value, |m| m.max(value)));
                    let sum: f64 = self.latency_samples.iter().sum();
                    self.avg_latency = Some(sum / self.latency_samples.len() as f64);
                    let probe_idx = self.latency_points.len() as f64 + 1.0;
                    self.latency_points.push((probe_idx, value));
                    if self.phase == TestPhase::Latency {
                        self.status = "Checking response latency".to_string();
                    }
                }
            }
            SpeedUpdate::Complete(result) => {
                self.phase = TestPhase::Complete;
                self.progress = 1.0;
                self.running = false;
                self.error = None;
                self.download_mbps = valid_measurement(result.download_mbps);
                self.upload_mbps = valid_measurement(result.upload_mbps);
                self.latency_ms = valid_measurement(result.latency_ms);
                if let Some(dl) = self.download_mbps {
                    self.peak_download = Some(self.peak_download.map_or(dl, |p| p.max(dl)));
                    if self.download_points.is_empty() {
                        self.download_points.push((0.0, dl));
                        self.download_points.push((1.0, dl));
                    }
                }
                if let Some(ul) = self.upload_mbps {
                    self.peak_upload = Some(self.peak_upload.map_or(ul, |p| p.max(ul)));
                    if self.upload_points.is_empty() {
                        self.upload_points.push((1.0, ul));
                        self.upload_points.push((2.0, ul));
                    }
                }
                if let Some(lat) = self.latency_ms {
                    self.min_latency = Some(self.min_latency.map_or(lat, |m| m.min(lat)));
                    self.max_latency = Some(self.max_latency.map_or(lat, |m| m.max(lat)));
                    self.avg_latency = Some(self.avg_latency.unwrap_or(lat));
                    if self.latency_points.is_empty() {
                        self.latency_points.push((1.0, lat));
                    }
                }
                self.status = phase_status(TestPhase::Complete).to_string();
                self.result = Some(result);
            }
            SpeedUpdate::Failed(message) => {
                self.phase = TestPhase::Failed;
                self.running = false;
                self.error = Some(if message.trim().is_empty() {
                    "The speed test stopped before completing.".to_string()
                } else {
                    message
                });
                self.status = phase_status(TestPhase::Failed).to_string();
            }
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn set_running(&mut self, running: bool) {
        self.running = running;
        if running {
            self.error = None;
            if self.phase == TestPhase::Idle {
                self.status = "Starting speed test".to_string();
            }
        } else if !matches!(self.phase, TestPhase::Complete | TestPhase::Failed) {
            self.status = "Test stopped · press s to start again".to_string();
        }
    }
}

/// Render the complete Fast.com dashboard.
pub fn ui(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    // A compact branch avoids layouts with competing minimums on narrow panes and
    // keeps every widget inside a valid rectangle, including a one-line terminal.
    if area.width < 54 || area.height < 14 {
        render_compact(frame, area, app);
    } else {
        render_dashboard(frame, area, app);
    }
}

fn render_dashboard(frame: &mut Frame, area: Rect, app: &App) {
    let shell = Block::default()
        .title(Line::from(vec![
            Span::styled(
                " FAST",
                Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
            ),
            Span::styled("/", Style::default().fg(AMBER)),
            Span::styled(
                "LINK ",
                Style::default().fg(INK).add_modifier(Modifier::BOLD),
            ),
            Span::styled("· Internet Speed Test", Style::default().fg(MUTED)),
        ]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PANEL_BORDER))
        .style(Style::default().bg(BG));
    let inner = shell.inner(area);
    frame.render_widget(shell, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let (hero_height, graph_min) = if inner.height >= 26 {
        (8, 10)
    } else if inner.height >= 20 {
        (8, 7)
    } else if inner.height >= 16 {
        (6, 5)
    } else {
        (5, 4)
    };
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),           // Header
            Constraint::Length(hero_height), // Hero overview
            Constraint::Min(graph_min),      // Real-time Telemetry Line Graphs
            Constraint::Length(3),           // Secondary metric cards
            Constraint::Length(1),           // Footer
        ])
        .split(inner);

    render_header(frame, sections[0], app);
    render_hero(frame, sections[1], app);
    render_telemetry_graphs(frame, sections[2], app);
    render_secondary_cards(frame, sections[3], app);
    render_footer(frame, sections[4], app);
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let activity = if app.running {
        "● LIVE MEASUREMENT"
    } else {
        "STANDBY"
    };
    let activity_color = if app.running { GREEN } else { MUTED };

    let status_text = app.error.as_deref().unwrap_or(&app.status);
    let status_color = if app.error.is_some() { RED } else { INK };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    let status_line = Paragraph::new(Line::from(vec![
        Span::styled("Status: ", Style::default().fg(MUTED)),
        Span::styled(status_text, Style::default().fg(status_color)),
    ]))
    .wrap(Wrap { trim: true });
    frame.render_widget(status_line, cols[0]);

    let badge = Paragraph::new(Line::from(Span::styled(
        format!("[ {activity} ]"),
        Style::default()
            .fg(activity_color)
            .add_modifier(Modifier::BOLD),
    )))
    .alignment(ratatui::layout::Alignment::Right);
    frame.render_widget(badge, cols[1]);
}

fn render_hero(frame: &mut Frame, area: Rect, app: &App) {
    let hero_border_color = if app.error.is_some() {
        RED
    } else if app.phase == TestPhase::Complete {
        GREEN
    } else if app.running {
        match app.phase {
            TestPhase::Download => CYAN,
            TestPhase::Upload => BLUE,
            TestPhase::Latency => AMBER,
            _ => CYAN,
        }
    } else {
        PANEL_BORDER
    };

    let hero_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(hero_border_color))
        .style(Style::default().bg(PANEL));
    let inner = hero_block.inner(area);
    frame.render_widget(hero_block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let (hero_title, val_str, unit_str, hero_color, subtitle) = match app.phase {
        TestPhase::Idle => (
            "INTERNET SPEED TEST",
            "0.0".to_string(),
            "Mbps",
            MUTED,
            "Press s or Enter to start speed measurement",
        ),
        TestPhase::FetchingTargets => (
            "FINDING TEST TARGETS",
            "—".to_string(),
            "Mbps",
            CYAN,
            "Connecting to nearest Fast.com measurement servers...",
        ),
        TestPhase::Download => (
            "DOWNLOAD SPEED",
            format_measurement_or_dash(app.download_mbps),
            "Mbps",
            CYAN,
            "Streaming payloads to measure download throughput",
        ),
        TestPhase::Upload => (
            "UPLOAD SPEED",
            format_measurement_or_dash(app.upload_mbps),
            "Mbps",
            BLUE,
            "Sending payloads to measure upload throughput",
        ),
        TestPhase::Latency => (
            "LATENCY / PING",
            format_measurement_or_dash(app.latency_ms),
            "ms",
            AMBER,
            "Checking round-trip server latency",
        ),
        TestPhase::Complete => (
            "YOUR INTERNET SPEED",
            format_measurement_or_dash(app.download_mbps),
            "Mbps",
            GREEN,
            "Speed measurement complete",
        ),
        TestPhase::Failed => (
            "MEASUREMENT FAILED",
            "—".to_string(),
            "Mbps",
            RED,
            app.error
                .as_deref()
                .unwrap_or("The speed test encountered an error"),
        ),
    };

    if inner.height >= 5 {
        let show_subtitle = inner.height >= 7;
        let digits_height = if inner.height >= 6 { 3 } else { 1 };
        let mut constraints = vec![
            Constraint::Length(1),             // Title
            Constraint::Length(digits_height), // Digits
        ];
        if show_subtitle {
            constraints.push(Constraint::Length(1)); // Subtitle
        }
        constraints.push(Constraint::Length(1)); // Progress Bar
        constraints.push(Constraint::Min(0)); // Stepper (if space)

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);

        let title_line = Paragraph::new(Line::from(Span::styled(
            hero_title,
            Style::default()
                .fg(if app.running { hero_color } else { MUTED })
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(title_line, rows[0]);

        if rows[1].height >= 3 && rows[1].width >= 20 {
            let big_lines = build_big_number_lines(&val_str, unit_str, hero_color);
            let digit_para =
                Paragraph::new(big_lines).alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(digit_para, rows[1]);
        } else {
            let fallback_line = Paragraph::new(Line::from(vec![
                Span::styled(
                    &val_str,
                    Style::default().fg(hero_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {unit_str}"), Style::default().fg(MUTED)),
            ]))
            .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(fallback_line, rows[1]);
        }

        let next_idx = if show_subtitle {
            let sub_line = Paragraph::new(Line::from(Span::styled(
                subtitle,
                Style::default().fg(if app.error.is_some() { RED } else { MUTED }),
            )))
            .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(sub_line, rows[2]);
            3
        } else {
            2
        };

        let gauge_color = if app.error.is_some() {
            RED
        } else if app.phase == TestPhase::Complete {
            GREEN
        } else if app.running {
            hero_color
        } else {
            MUTED
        };
        let gauge_pct = (app.progress * 100.0).round().clamp(0.0, 100.0) as u16;
        let gauge_label = format!("{} {:>3}%", phase_name(app.phase), gauge_pct);
        let gauge = Gauge::default()
            .gauge_style(
                Style::default()
                    .fg(gauge_color)
                    .bg(PANEL_ALT)
                    .add_modifier(Modifier::BOLD),
            )
            .percent(gauge_pct)
            .label(gauge_label);
        frame.render_widget(gauge, rows[next_idx]);

        if rows.len() > next_idx + 1 && rows[next_idx + 1].height > 0 {
            render_stepper(frame, rows[next_idx + 1], app);
        }
    } else {
        let hero_line = Paragraph::new(Line::from(vec![
            Span::styled(
                hero_title,
                Style::default().fg(hero_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" : ", Style::default().fg(MUTED)),
            Span::styled(
                &val_str,
                Style::default().fg(hero_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {unit_str}"), Style::default().fg(MUTED)),
        ]))
        .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(hero_line, inner);
    }
}
fn render_telemetry_graphs(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if area.width >= 70 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);

        render_speed_chart(frame, cols[0], app);
        render_latency_chart(frame, cols[1], app);
    } else {
        render_speed_chart(frame, area, app);
    }
}

fn render_speed_chart(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let max_time = app
        .download_points
        .iter()
        .chain(app.upload_points.iter())
        .map(|(t, _)| *t)
        .fold(0.0f64, |acc, t| acc.max(t))
        .max(5.0);

    let max_speed = app
        .download_points
        .iter()
        .chain(app.upload_points.iter())
        .map(|(_, s)| *s)
        .fold(0.0f64, |acc, s| acc.max(s))
        .max(10.0);

    let y_max = round_up_chart_max(max_speed);
    let x_bounds = [0.0, max_time];
    let y_bounds = [0.0, y_max];

    let x_labels = vec![
        Span::styled("0s", Style::default().fg(MUTED)),
        Span::styled(
            format!("{:.1}s", max_time / 2.0),
            Style::default().fg(MUTED),
        ),
        Span::styled(format!("{:.1}s", max_time), Style::default().fg(MUTED)),
    ];

    let y_labels = vec![
        Span::styled("0", Style::default().fg(MUTED)),
        Span::styled(format!("{:.0}", y_max / 2.0), Style::default().fg(MUTED)),
        Span::styled(format!("{:.0} Mbps", y_max), Style::default().fg(MUTED)),
    ];

    let mut datasets = Vec::new();
    if !app.download_points.is_empty() {
        datasets.push(
            Dataset::default()
                .name("Download")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(CYAN))
                .data(&app.download_points),
        );
    }
    if !app.upload_points.is_empty() {
        datasets.push(
            Dataset::default()
                .name("Upload")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(BLUE))
                .data(&app.upload_points),
        );
    }

    let peak_info = if area.width >= 52 {
        match (app.peak_download, app.peak_upload) {
            (Some(dl), Some(ul)) => format!(" (Peak: ↓{dl:.1} / ↑{ul:.1} Mbps)"),
            (Some(dl), None) => format!(" (Peak: ↓{dl:.1} Mbps)"),
            (None, Some(ul)) => format!(" (Peak: ↑{ul:.1} Mbps)"),
            (None, None) => String::new(),
        }
    } else if area.width >= 35 {
        match (app.peak_download, app.peak_upload) {
            (Some(dl), Some(ul)) => format!(" (↓{dl:.0}/↑{ul:.0}M)"),
            (Some(dl), None) => format!(" (↓{dl:.0}M)"),
            (None, Some(ul)) => format!(" (↑{ul:.0}M)"),
            (None, None) => String::new(),
        }
    } else {
        String::new()
    };
    let title = Line::from(vec![
        Span::styled(
            " SPEED FLUCTUATION ",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(peak_info, Style::default().fg(MUTED)),
    ]);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PANEL_BORDER))
        .style(Style::default().bg(PANEL));

    if datasets.is_empty() {
        let msg = if app.running {
            "Waiting for throughput measurements..."
        } else {
            "Real-time speed fluctuation graph · press s to start"
        };
        let placeholder = Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(MUTED))))
            .block(block)
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(placeholder, area);
        return;
    }

    let chart = Chart::new(datasets)
        .block(block)
        .x_axis(
            Axis::default()
                .title(Span::styled("Time", Style::default().fg(MUTED)))
                .style(Style::default().fg(PANEL_BORDER))
                .bounds(x_bounds)
                .labels(x_labels),
        )
        .y_axis(
            Axis::default()
                .title(Span::styled("Mbps", Style::default().fg(MUTED)))
                .style(Style::default().fg(PANEL_BORDER))
                .bounds(y_bounds)
                .labels(y_labels),
        );

    frame.render_widget(chart, area);
}

fn render_latency_chart(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let max_probes = app
        .latency_points
        .iter()
        .map(|(p, _)| *p)
        .fold(0.0f64, |acc, p| acc.max(p))
        .max(8.0);

    let max_latency = app
        .latency_points
        .iter()
        .map(|(_, l)| *l)
        .fold(0.0f64, |acc, l| acc.max(l))
        .max(20.0);

    let y_max = round_up_chart_max(max_latency);
    let x_bounds = [1.0, max_probes];
    let y_bounds = [0.0, y_max];

    let x_labels = vec![
        Span::styled("p1", Style::default().fg(MUTED)),
        Span::styled(
            format!("p{:.0}", (1.0 + max_probes) / 2.0),
            Style::default().fg(MUTED),
        ),
        Span::styled(format!("p{:.0}", max_probes), Style::default().fg(MUTED)),
    ];

    let y_labels = vec![
        Span::styled("0", Style::default().fg(MUTED)),
        Span::styled(format!("{:.0}", y_max / 2.0), Style::default().fg(MUTED)),
        Span::styled(format!("{:.0} ms", y_max), Style::default().fg(MUTED)),
    ];

    let mut datasets = Vec::new();
    if !app.latency_points.is_empty() {
        datasets.push(
            Dataset::default()
                .name("Latency")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(AMBER))
                .data(&app.latency_points),
        );
    }

    let (chart_heading, stats_info) = if area.width >= 42 {
        let stats = match (app.min_latency, app.avg_latency, app.max_latency) {
            (Some(min), Some(avg), Some(max)) => {
                format!(" (Min: {min:.1} / Avg: {avg:.1} / Max: {max:.1} ms)")
            }
            _ => String::new(),
        };
        (" LATENCY / JITTER ", stats)
    } else if area.width >= 28 {
        let stats = match app.avg_latency {
            Some(avg) => format!(" (~{avg:.0}ms)"),
            None => String::new(),
        };
        (" LATENCY ", stats)
    } else {
        (" LATENCY ", String::new())
    };

    let title = Line::from(vec![
        Span::styled(
            chart_heading,
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        ),
        Span::styled(stats_info, Style::default().fg(MUTED)),
    ]);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PANEL_BORDER))
        .style(Style::default().bg(PANEL));

    if datasets.is_empty() {
        let msg = if app.running {
            "Probing ping latency..."
        } else {
            "Real-time latency fluctuation graph"
        };
        let placeholder = Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(MUTED))))
            .block(block)
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(placeholder, area);
        return;
    }

    let chart = Chart::new(datasets)
        .block(block)
        .x_axis(
            Axis::default()
                .title(Span::styled("Probe", Style::default().fg(MUTED)))
                .style(Style::default().fg(PANEL_BORDER))
                .bounds(x_bounds)
                .labels(x_labels),
        )
        .y_axis(
            Axis::default()
                .title(Span::styled("ms", Style::default().fg(MUTED)))
                .style(Style::default().fg(PANEL_BORDER))
                .bounds(y_bounds)
                .labels(y_labels),
        );

    frame.render_widget(chart, area);
}

fn round_up_chart_max(val: f64) -> f64 {
    if !val.is_finite() || val <= 0.0 {
        return 10.0;
    }
    let padded = val * 1.15;
    if padded <= 10.0 {
        10.0
    } else if padded <= 25.0 {
        25.0
    } else if padded <= 50.0 {
        50.0
    } else if padded <= 100.0 {
        100.0
    } else if padded <= 250.0 {
        250.0
    } else if padded <= 500.0 {
        500.0
    } else if padded <= 1000.0 {
        1000.0
    } else {
        (padded / 500.0).ceil() * 500.0
    }
}

fn render_stepper(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let step_state = |target_phase: TestPhase| -> (&'static str, Color, bool) {
        match target_phase {
            TestPhase::FetchingTargets => match app.phase {
                TestPhase::Idle => ("○ Targets", MUTED, false),
                TestPhase::FetchingTargets => ("▶ Targets", CYAN, true),
                _ => ("✓ Targets", GREEN, false),
            },
            TestPhase::Download => match app.phase {
                TestPhase::Idle | TestPhase::FetchingTargets => ("○ Download", MUTED, false),
                TestPhase::Download => ("▶ Download", CYAN, true),
                TestPhase::Upload | TestPhase::Latency | TestPhase::Complete => {
                    ("✓ Download", GREEN, false)
                }
                TestPhase::Failed => ("✕ Download", RED, false),
            },
            TestPhase::Upload => match app.phase {
                TestPhase::Idle | TestPhase::FetchingTargets | TestPhase::Download => {
                    ("○ Upload", MUTED, false)
                }
                TestPhase::Upload => ("▶ Upload", BLUE, true),
                TestPhase::Latency | TestPhase::Complete => ("✓ Upload", GREEN, false),
                TestPhase::Failed => ("○ Upload", MUTED, false),
            },
            TestPhase::Latency => match app.phase {
                TestPhase::Complete => ("✓ Latency", GREEN, false),
                TestPhase::Latency => ("▶ Latency", AMBER, true),
                TestPhase::Failed => ("○ Latency", MUTED, false),
                _ => ("○ Latency", MUTED, false),
            },
            _ => ("○", MUTED, false),
        }
    };

    let (s1, c1, b1) = step_state(TestPhase::FetchingTargets);
    let (s2, c2, b2) = step_state(TestPhase::Download);
    let (s3, c3, b3) = step_state(TestPhase::Upload);
    let (s4, c4, b4) = step_state(TestPhase::Latency);

    let format_step = |text: &'static str, color: Color, bold: bool| -> Span<'static> {
        let mut style = Style::default().fg(color);
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        Span::styled(text, style)
    };

    let sep = Span::styled("  ───  ", Style::default().fg(PANEL_BORDER));

    let stepper_line = Paragraph::new(Line::from(vec![
        format_step(s1, c1, b1),
        sep.clone(),
        format_step(s2, c2, b2),
        sep.clone(),
        format_step(s3, c3, b3),
        sep,
        format_step(s4, c4, b4),
    ]))
    .alignment(ratatui::layout::Alignment::Center);

    frame.render_widget(stepper_line, area);
}

fn render_secondary_cards(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let cards = if area.width >= 60 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(34),
                Constraint::Percentage(33),
                Constraint::Percentage(33),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Length(2),
            ])
            .split(area)
    };

    let dl_sub = app.peak_download.map(|p| format!("peak {p:.1}"));
    let ul_sub = app.peak_upload.map(|p| format!("peak {p:.1}"));
    let lat_sub = match (app.min_latency, app.max_latency) {
        (Some(min), Some(max)) => Some(format!("min {min:.1} / max {max:.1}")),
        _ => None,
    };

    render_metric_card(
        frame,
        cards[0],
        " ⬇ DOWNLOAD ",
        app.download_mbps,
        "Mbps",
        dl_sub.as_deref(),
        CYAN,
        app.phase == TestPhase::Download,
        app.phase == TestPhase::Complete || app.download_mbps.is_some(),
    );
    render_metric_card(
        frame,
        cards[1],
        " ⬆ UPLOAD ",
        app.upload_mbps,
        "Mbps",
        ul_sub.as_deref(),
        BLUE,
        app.phase == TestPhase::Upload,
        app.phase == TestPhase::Complete || app.upload_mbps.is_some(),
    );
    render_metric_card(
        frame,
        cards[2],
        " ⟳ LATENCY ",
        app.latency_ms,
        "ms",
        lat_sub.as_deref(),
        AMBER,
        app.phase == TestPhase::Latency,
        app.phase == TestPhase::Complete || app.latency_ms.is_some(),
    );
}

#[allow(clippy::too_many_arguments)]
fn render_metric_card(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: Option<f64>,
    unit: &str,
    sub_info: Option<&str>,
    accent_color: Color,
    is_active: bool,
    is_done: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let border_color = if is_active {
        accent_color
    } else {
        PANEL_BORDER
    };

    let title_style = if is_active {
        Style::default()
            .fg(accent_color)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD)
    };

    let block = Block::default()
        .title(Span::styled(label, title_style))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let number = match value {
        Some(v) => format_measurement(v),
        None => "—".to_string(),
    };

    let value_color = if is_active {
        accent_color
    } else if is_done {
        INK
    } else {
        MUTED
    };

    let badge = if is_active {
        Span::styled(
            " [LIVE]",
            Style::default()
                .fg(accent_color)
                .add_modifier(Modifier::BOLD),
        )
    } else if is_done && value.is_some() {
        Span::styled(" [DONE]", Style::default().fg(GREEN))
    } else {
        Span::raw("")
    };

    let lines = if let Some(sub) = sub_info {
        if inner.height >= 2 {
            vec![
                Line::from(vec![
                    Span::styled(
                        number,
                        Style::default()
                            .fg(value_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!(" {unit}"), Style::default().fg(MUTED)),
                    badge,
                ]),
                Line::from(Span::styled(sub, Style::default().fg(MUTED))),
            ]
        } else {
            vec![Line::from(vec![
                Span::styled(
                    number,
                    Style::default()
                        .fg(value_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {unit}"), Style::default().fg(MUTED)),
                badge,
            ])]
        }
    } else {
        vec![Line::from(vec![
            Span::styled(
                number,
                Style::default()
                    .fg(value_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {unit}"), Style::default().fg(MUTED)),
            badge,
        ])]
    };

    let content = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(content, inner);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let action = if app.running {
        "test in progress..."
    } else if app.result.is_some() || matches!(app.phase, TestPhase::Complete | TestPhase::Failed) {
        "s / Enter restart"
    } else {
        "s / Enter start"
    };

    let footer = Paragraph::new(Line::from(vec![
        Span::styled("[ ", Style::default().fg(AMBER)),
        Span::styled(
            action,
            Style::default().fg(INK).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ]", Style::default().fg(AMBER)),
        Span::styled("      ", Style::default()),
        Span::styled("[ q / Esc quit ]", Style::default().fg(MUTED)),
    ]))
    .alignment(ratatui::layout::Alignment::Center)
    .style(Style::default().bg(BG));
    frame.render_widget(footer, area);
}

fn render_compact(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "FAST",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "/LINK",
            Style::default().fg(INK).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(MUTED)),
        Span::styled(
            phase_name(app.phase),
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        ),
    ]));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    frame.render_widget(title, rows[0]);

    if rows[1].height > 0 {
        let content = vec![
            Line::from(vec![
                Span::styled("↓ ", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format_measurement_or_dash(app.download_mbps),
                    Style::default().fg(INK).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Mbps   ", Style::default().fg(MUTED)),
                Span::styled("↑ ", Style::default().fg(BLUE).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format_measurement_or_dash(app.upload_mbps),
                    Style::default().fg(INK).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Mbps   ", Style::default().fg(MUTED)),
                Span::styled(
                    "⟳ ",
                    Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format_measurement_or_dash(app.latency_ms),
                    Style::default().fg(INK).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ms", Style::default().fg(MUTED)),
            ]),
            Line::from(vec![
                Span::styled("Progress: ", Style::default().fg(MUTED)),
                Span::styled(
                    format!("{:>3.0}%", app.progress * 100.0),
                    Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  ·  ", Style::default().fg(MUTED)),
                Span::styled(
                    app.error.as_deref().unwrap_or(&app.status),
                    Style::default().fg(if app.error.is_some() { RED } else { MUTED }),
                ),
            ]),
        ];
        frame.render_widget(
            Paragraph::new(content)
                .wrap(Wrap { trim: true })
                .style(Style::default().bg(PANEL)),
            rows[1],
        );
    }

    let action = if app.running {
        "test in progress..."
    } else if app.result.is_some() || matches!(app.phase, TestPhase::Complete | TestPhase::Failed) {
        "s/Enter restart"
    } else {
        "s/Enter start"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                action,
                Style::default().fg(INK).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ·  q quit", Style::default().fg(MUTED)),
        ])),
        rows[2],
    );
}

fn digit_glyph(ch: char) -> [&'static str; 3] {
    match ch {
        '0' => ["█▀▀█", "█  █", "█▄▄█"],
        '1' => [" ▄█ ", "  █ ", " ▄█▄"],
        '2' => ["█▀▀█", " ▄▄▀", "█▄▄▄"],
        '3' => ["█▀▀█", "  ▀▄", "█▄▄█"],
        '4' => ["█  █", "█▀▀█", "   █"],
        '5' => ["█▀▀▀", "▀▀▀█", "█▄▄█"],
        '6' => ["█▀▀█", "█▀▀▄", "█▄▄█"],
        '7' => ["▀▀▀█", "  █ ", "  █ "],
        '8' => ["█▀▀█", "█▀▀█", "█▄▄█"],
        '9' => ["█▀▀█", "▀▀▀█", "  ▄█"],
        '.' => [" ", " ", "█"],
        '-' | '—' => ["    ", "▀▀▀▀", "    "],
        ' ' => ["  ", "  ", "  "],
        _ => [" ", " ", " "],
    }
}

fn build_big_number_lines(text: &str, unit: &str, color: Color) -> Vec<Line<'static>> {
    let mut row0 = String::new();
    let mut row1 = String::new();
    let mut row2 = String::new();

    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        let glyph = digit_glyph(ch);
        row0.push_str(glyph[0]);
        row1.push_str(glyph[1]);
        row2.push_str(glyph[2]);
        if chars.peek().is_some() {
            row0.push(' ');
            row1.push(' ');
            row2.push(' ');
        }
    }

    let unit_str = format!("  {unit}");
    let pad = " ".repeat(unit_str.len());

    vec![
        Line::from(vec![
            Span::styled(
                row0,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(pad.clone()),
        ]),
        Line::from(vec![
            Span::styled(
                row1,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(pad),
        ]),
        Line::from(vec![
            Span::styled(
                row2,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                unit_str,
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            ),
        ]),
    ]
}

fn phase_name(phase: TestPhase) -> &'static str {
    match phase {
        TestPhase::Idle => "Ready",
        TestPhase::FetchingTargets => "Finding targets",
        TestPhase::Download => "Download",
        TestPhase::Upload => "Upload",
        TestPhase::Latency => "Latency",
        TestPhase::Complete => "Complete",
        TestPhase::Failed => "Failed",
    }
}

fn phase_status(phase: TestPhase) -> &'static str {
    match phase {
        TestPhase::Idle => ready_status(),
        TestPhase::FetchingTargets => "Finding the nearest Fast.com test targets",
        TestPhase::Download => "Measuring download throughput",
        TestPhase::Upload => "Measuring upload throughput",
        TestPhase::Latency => "Checking response latency",
        TestPhase::Complete => "Measurement complete · press s or Enter to run again",
        TestPhase::Failed => "Measurement failed · press s or Enter to retry",
    }
}

const fn ready_status() -> &'static str {
    "Ready · press s to start a measurement"
}

fn valid_measurement(value: f64) -> Option<f64> {
    value
        .is_finite()
        .then_some(value)
        .filter(|value| *value >= 0.0)
}

fn format_measurement(value: f64) -> String {
    if value >= 100.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

fn format_measurement_or_dash(value: Option<f64>) -> String {
    value
        .map(format_measurement)
        .unwrap_or_else(|| "—".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn test_app_transitions_and_updates() {
        let mut app = App::new();
        assert_eq!(app.phase, TestPhase::Idle);
        assert!(!app.is_running());

        app.apply_update(SpeedUpdate::Phase(TestPhase::FetchingTargets));
        assert_eq!(app.phase, TestPhase::FetchingTargets);
        assert!(app.is_running());

        app.apply_update(SpeedUpdate::Phase(TestPhase::Download));
        app.apply_update(SpeedUpdate::DownloadMbps(124.5));
        app.apply_update(SpeedUpdate::Progress(0.35));
        assert_eq!(app.download_mbps, Some(124.5));
        assert_eq!(app.progress, 0.35);

        app.apply_update(SpeedUpdate::Phase(TestPhase::Upload));
        app.apply_update(SpeedUpdate::UploadMbps(42.1));
        app.apply_update(SpeedUpdate::Progress(0.70));
        assert_eq!(app.upload_mbps, Some(42.1));

        app.apply_update(SpeedUpdate::Phase(TestPhase::Latency));
        app.apply_update(SpeedUpdate::LatencyMs(15.2));
        assert_eq!(app.latency_ms, Some(15.2));

        app.apply_update(SpeedUpdate::Complete(TestResult {
            download_mbps: 124.5,
            upload_mbps: 42.1,
            latency_ms: 15.2,
        }));
        assert_eq!(app.phase, TestPhase::Complete);
        assert_eq!(app.progress, 1.0);
        assert!(!app.is_running());
        assert!(app.result.is_some());

        app.reset();
        assert_eq!(app.phase, TestPhase::Idle);
        assert_eq!(app.progress, 0.0);
        assert_eq!(app.download_mbps, None);
        assert_eq!(app.download_points.len(), 0);
        assert_eq!(app.upload_points.len(), 0);
        assert_eq!(app.latency_points.len(), 0);
    }

    #[test]
    fn test_telemetry_points_and_peak_stats() {
        let mut app = App::new();
        app.apply_update(SpeedUpdate::Phase(TestPhase::FetchingTargets));
        assert_eq!(app.download_points.len(), 0);

        app.apply_update(SpeedUpdate::Phase(TestPhase::Download));
        app.apply_update(SpeedUpdate::DownloadMbps(100.0));
        app.apply_update(SpeedUpdate::DownloadMbps(250.0));
        app.apply_update(SpeedUpdate::DownloadMbps(200.0));
        assert_eq!(app.peak_download, Some(250.0));
        assert_eq!(app.download_points.len(), 3);

        app.apply_update(SpeedUpdate::Phase(TestPhase::Upload));
        app.apply_update(SpeedUpdate::UploadMbps(30.0));
        app.apply_update(SpeedUpdate::UploadMbps(75.0));
        assert_eq!(app.peak_upload, Some(75.0));
        assert_eq!(app.upload_points.len(), 2);

        app.apply_update(SpeedUpdate::Phase(TestPhase::Latency));
        app.apply_update(SpeedUpdate::LatencyMs(20.0));
        app.apply_update(SpeedUpdate::LatencyMs(10.0));
        app.apply_update(SpeedUpdate::LatencyMs(15.0));
        assert_eq!(app.min_latency, Some(10.0));
        assert_eq!(app.max_latency, Some(20.0));
        assert_eq!(app.avg_latency, Some(15.0));
        assert_eq!(app.latency_points.len(), 3);
    }

    #[test]
    fn test_round_up_chart_max() {
        assert_eq!(round_up_chart_max(0.0), 10.0);
        assert_eq!(round_up_chart_max(-5.0), 10.0);
        assert_eq!(round_up_chart_max(f64::NAN), 10.0);
        assert_eq!(round_up_chart_max(8.0), 10.0);
        assert_eq!(round_up_chart_max(15.0), 25.0);
        assert_eq!(round_up_chart_max(35.0), 50.0);
        assert_eq!(round_up_chart_max(75.0), 100.0);
        assert_eq!(round_up_chart_max(180.0), 250.0);
        assert_eq!(round_up_chart_max(380.0), 500.0);
        assert_eq!(round_up_chart_max(800.0), 1000.0);
        assert_eq!(round_up_chart_max(1200.0), 1500.0);
    }
    #[test]
    fn test_digit_glyphs_and_big_number_lines() {
        for ch in "0123456789.-— ".chars() {
            let glyph = digit_glyph(ch);
            assert_eq!(glyph.len(), 3);
            assert!(!glyph[0].is_empty());
        }

        let lines = build_big_number_lines("384.2", "Mbps", CYAN);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_ui_rendering_various_dimensions_and_phases() {
        let sizes = [
            (0, 0),
            (1, 1),
            (20, 5),
            (53, 13),
            (80, 24),
            (120, 40),
            (200, 60),
        ];
        let phases = [
            TestPhase::Idle,
            TestPhase::FetchingTargets,
            TestPhase::Download,
            TestPhase::Upload,
            TestPhase::Latency,
            TestPhase::Complete,
            TestPhase::Failed,
        ];

        for (w, h) in sizes {
            for phase in phases {
                let mut app = App::new();
                app.phase = phase;
                app.running = matches!(
                    phase,
                    TestPhase::FetchingTargets
                        | TestPhase::Download
                        | TestPhase::Upload
                        | TestPhase::Latency
                );
                app.download_mbps = Some(250.0);
                app.upload_mbps = Some(50.0);
                app.latency_ms = Some(12.0);
                app.progress = 0.5;
                if phase == TestPhase::Failed {
                    app.error = Some("Connection refused".to_string());
                }

                let backend = TestBackend::new(w, h);
                let mut terminal = Terminal::new(backend).unwrap();
                terminal.draw(|f| ui(f, &app)).unwrap();
            }
        }
    }

    #[test]
    fn test_rendered_buffer_layout() {
        let mut app = App::new();
        app.phase = TestPhase::Download;
        app.running = true;
        app.download_mbps = Some(348.5);
        app.peak_download = Some(380.0);
        app.progress = 0.45;
        app.status = "Measuring download throughput".to_string();
        app.download_points = vec![
            (0.0, 50.0),
            (0.5, 150.0),
            (1.0, 280.0),
            (1.5, 340.0),
            (2.0, 380.0),
            (2.5, 348.5),
        ];
        app.latency_points = vec![
            (1.0, 15.2),
            (2.0, 14.1),
            (3.0, 18.0),
            (4.0, 13.5),
            (5.0, 14.0),
        ];
        app.min_latency = Some(13.5);
        app.avg_latency = Some(14.9);
        app.max_latency = Some(18.0);

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| ui(f, &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        println!("\n=== DOWNLOAD PHASE (80x24) ===");
        for y in 0..buffer.area.height {
            let mut line = String::new();
            for x in 0..buffer.area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            println!("{line}");
        }

        // Complete phase
        app.phase = TestPhase::Complete;
        app.running = false;
        app.download_mbps = Some(348.5);
        app.upload_mbps = Some(84.2);
        app.latency_ms = Some(14.0);
        app.upload_points = vec![
            (3.0, 20.0),
            (3.5, 60.0),
            (4.0, 84.2),
            (4.5, 88.0),
            (5.0, 84.2),
        ];
        app.peak_upload = Some(88.0);
        app.progress = 1.0;
        app.status = "Measurement complete · press s or Enter to run again".to_string();
        terminal.draw(|f| ui(f, &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        println!("\n=== COMPLETE PHASE (80x24) ===");
        for y in 0..buffer.area.height {
            let mut line = String::new();
            for x in 0..buffer.area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            println!("{line}");
        }
    }
}
