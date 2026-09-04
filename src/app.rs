use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, Gauge, GraphType, Paragraph, Wrap},
    Frame,
};

use crate::speed::{SpeedUpdate, TestPhase, TestResult};

const BG: Color = Color::Rgb(10, 14, 23);
const PANEL: Color = Color::Rgb(15, 24, 38);
const PANEL_ALT: Color = Color::Rgb(22, 35, 54);
const PANEL_BORDER: Color = Color::Rgb(30, 45, 65);
const INK: Color = Color::Rgb(240, 246, 252);
const MUTED: Color = Color::Rgb(130, 145, 165);
const DIM: Color = Color::Rgb(80, 95, 115);
const CYAN: Color = Color::Rgb(0, 212, 255);
const PURPLE: Color = Color::Rgb(176, 110, 255);
const AMBER: Color = Color::Rgb(255, 179, 0);
const RED: Color = Color::Rgb(246, 104, 112);
const GREEN: Color = Color::Rgb(0, 230, 118);

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
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    // A compact branch avoids layouts with competing minimums on narrow panes and
    // keeps every widget inside a valid rectangle, including a one-line terminal.
    if area.width < 54 || area.height < 14 {
        render_compact(frame, area, app);
    } else if area.width < 100 || area.height < 28 {
        render_intermediate(frame, area, app);
    } else {
        render_dashboard(frame, area, app);
    }
}

fn render_dashboard(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top Header Row
            Constraint::Length(1), // Thin Horizontal Divider Rule
            Constraint::Length(9), // Centered Hero Section
            Constraint::Length(3), // Metric Triplet (Download, Upload, Latency)
            Constraint::Min(10),   // Middle Row: 3 Equal Bordered Chart Panels
            Constraint::Length(7), // Lower Bordered Panel: RESULTS
            Constraint::Length(1), // Separated Centered Green Success Footer
        ])
        .split(area);

    render_header(frame, vertical_chunks[0], app);
    render_divider(frame, vertical_chunks[1]);
    render_hero(frame, vertical_chunks[2], app);
    render_metric_triplet(frame, vertical_chunks[3], app);
    render_charts_row(frame, vertical_chunks[4], app);
    render_results(frame, vertical_chunks[5], app);
    render_footer(frame, vertical_chunks[6], app);
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(16),
            Constraint::Min(0),
            Constraint::Length(48),
        ])
        .split(area);

    let left = Paragraph::new(Line::from(vec![
        Span::styled(
            "FAST",
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        ),
        Span::styled("/", Style::default().fg(PANEL_BORDER)),
        Span::styled(
            "TEST",
            Style::default().fg(INK).add_modifier(Modifier::BOLD),
        ),
    ]));
    frame.render_widget(left, cols[0]);

    let center = Paragraph::new(Line::from(Span::styled(
        "Internet Speed Test",
        Style::default().fg(MUTED),
    )))
    .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(center, cols[1]);

    let header_copy =
        if app.result.is_some() || matches!(app.phase, TestPhase::Complete | TestPhase::Failed) {
            "Press 's' or Enter to test again · 'q' to quit"
        } else if app.is_running() {
            if cols[2].width >= 41 {
                "Press 's' or Enter to start · 'q' to quit"
            } else {
                "'q' to quit"
            }
        } else {
            "Press 's' or Enter to start · 'q' to quit"
        };

    let text = if (cols[2].width as usize) < header_copy.len() && cols[2].width >= 11 {
        "'q' to quit"
    } else if (cols[2].width as usize) < 11 {
        ""
    } else {
        header_copy
    };

    let right = Paragraph::new(Line::from(Span::styled(text, Style::default().fg(DIM))))
        .alignment(ratatui::layout::Alignment::Right);
    frame.render_widget(right, cols[2]);
}

fn render_divider(frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let rule = "─".repeat(area.width as usize);
    let para = Paragraph::new(Line::from(Span::styled(
        rule,
        Style::default().fg(PANEL_BORDER),
    )));
    frame.render_widget(para, area);
}

fn render_hero(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let (hero_title, val_str, unit_str, hero_color, status_text) = match app.phase {
        TestPhase::Idle => (
            "Your Internet Speed",
            "—".to_string(),
            "Mbps",
            MUTED,
            "Ready · press s to start".to_string(),
        ),
        TestPhase::FetchingTargets => (
            "Finding Test Targets",
            "—".to_string(),
            "Mbps",
            CYAN,
            "Connecting to Fast.com servers...".to_string(),
        ),
        TestPhase::Download => (
            "Download Speed",
            format_measurement_or_dash(app.download_mbps),
            "Mbps",
            CYAN,
            format!("Measuring download... {:>3.0}%", app.progress * 100.0),
        ),
        TestPhase::Upload => (
            "Upload Speed",
            format_measurement_or_dash(app.upload_mbps),
            "Mbps",
            PURPLE,
            format!("Measuring upload... {:>3.0}%", app.progress * 100.0),
        ),
        TestPhase::Latency => (
            "Latency / Ping",
            format_measurement_or_dash(app.latency_ms),
            "ms",
            AMBER,
            format!("Measuring latency... {:>3.0}%", app.progress * 100.0),
        ),
        TestPhase::Complete => (
            "Your Internet Speed",
            format_measurement_or_dash(app.download_mbps),
            "Mbps",
            GREEN,
            "Complete ✓ 100%".to_string(),
        ),
        TestPhase::Failed => (
            "Measurement Failed",
            "—".to_string(),
            "Mbps",
            RED,
            format!(
                "Failed · {}",
                app.error.as_deref().unwrap_or("Error encountered")
            ),
        ),
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title
            Constraint::Length(3), // Digits
            Constraint::Length(1), // Status row ("Complete ✓ 100%")
            Constraint::Length(1), // Full-width progress rule / gauge
            Constraint::Min(0),    // Padding
        ])
        .split(area);

    let title_line = Paragraph::new(Line::from(Span::styled(
        hero_title,
        Style::default()
            .fg(if app.phase == TestPhase::Complete {
                GREEN
            } else if app.running {
                hero_color
            } else {
                MUTED
            })
            .add_modifier(Modifier::BOLD),
    )))
    .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(title_line, rows[0]);

    if rows[1].height >= 3 && rows[1].width >= 16 {
        let big_lines = build_big_number_lines(&val_str, unit_str, hero_color);
        let digit_para = Paragraph::new(big_lines).alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(digit_para, rows[1]);
    } else {
        let fallback = Paragraph::new(Line::from(vec![
            Span::styled(
                &val_str,
                Style::default().fg(hero_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {unit_str}"), Style::default().fg(MUTED)),
        ]))
        .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(fallback, rows[1]);
    }

    let status_para = Paragraph::new(Line::from(Span::styled(
        status_text,
        Style::default()
            .fg(if app.phase == TestPhase::Complete {
                GREEN
            } else if app.error.is_some() {
                RED
            } else {
                MUTED
            })
            .add_modifier(if app.phase == TestPhase::Complete {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
    )))
    .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(status_para, rows[2]);

    let gauge_pct = (app.progress * 100.0).round().clamp(0.0, 100.0) as u16;
    let gauge_color = if app.error.is_some() {
        RED
    } else if app.phase == TestPhase::Complete {
        GREEN
    } else if app.running {
        hero_color
    } else {
        MUTED
    };
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(gauge_color).bg(PANEL_ALT))
        .percent(gauge_pct)
        .label("");
    frame.render_widget(gauge, rows[3]);
}

fn render_metric_triplet(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 3), // Download
            Constraint::Length(1),   // Separator
            Constraint::Ratio(1, 3), // Upload
            Constraint::Length(1),   // Separator
            Constraint::Ratio(1, 3), // Latency
        ])
        .split(area);

    let sep_height = area.height.min(3) as usize;
    let sep = Paragraph::new(vec![Line::from("│"); sep_height])
        .alignment(ratatui::layout::Alignment::Center)
        .style(Style::default().fg(PANEL_BORDER));
    frame.render_widget(sep.clone(), chunks[1]);
    frame.render_widget(sep, chunks[3]);

    // Download
    let dl_val = match app.download_mbps {
        Some(v) => format_measurement(v),
        None => "—".to_string(),
    };
    let dl_peak = app
        .peak_download
        .map(|p| format!("Peak: {:.2} Mbps", p))
        .unwrap_or_else(|| "Peak: —".to_string());
    let dl_lines = vec![
        Line::from(Span::styled(
            "DOWNLOAD",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                dl_val,
                Style::default()
                    .fg(if app.phase == TestPhase::Download {
                        CYAN
                    } else {
                        INK
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Mbps", Style::default().fg(MUTED)),
        ]),
        Line::from(Span::styled(dl_peak, Style::default().fg(MUTED))),
    ];
    frame.render_widget(
        Paragraph::new(dl_lines).alignment(ratatui::layout::Alignment::Center),
        chunks[0],
    );

    // Upload
    let ul_val = match app.upload_mbps {
        Some(v) => format_measurement(v),
        None => "—".to_string(),
    };
    let ul_peak = app
        .peak_upload
        .map(|p| format!("Peak: {:.2} Mbps", p))
        .unwrap_or_else(|| "Peak: —".to_string());
    let ul_lines = vec![
        Line::from(Span::styled(
            "UPLOAD",
            Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                ul_val,
                Style::default()
                    .fg(if app.phase == TestPhase::Upload {
                        PURPLE
                    } else {
                        INK
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Mbps", Style::default().fg(MUTED)),
        ]),
        Line::from(Span::styled(ul_peak, Style::default().fg(MUTED))),
    ];
    frame.render_widget(
        Paragraph::new(ul_lines).alignment(ratatui::layout::Alignment::Center),
        chunks[2],
    );

    // Latency
    let lat_val = match app.latency_ms {
        Some(v) => format_measurement(v),
        None => "—".to_string(),
    };
    let lat_stats = match (app.min_latency, app.max_latency) {
        (Some(min), Some(max)) => format!("Min: {min:.1} ms  Max: {max:.1} ms"),
        _ => "Min: —  Max: —".to_string(),
    };
    let lat_lines = vec![
        Line::from(Span::styled(
            "LATENCY",
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                lat_val,
                Style::default()
                    .fg(if app.phase == TestPhase::Latency {
                        AMBER
                    } else {
                        INK
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ms", Style::default().fg(MUTED)),
        ]),
        Line::from(Span::styled(lat_stats, Style::default().fg(MUTED))),
    ];
    frame.render_widget(
        Paragraph::new(lat_lines).alignment(ratatui::layout::Alignment::Center),
        chunks[4],
    );
}

fn render_charts_row(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(area);

    render_download_chart(frame, cols[0], app);
    render_upload_chart(frame, cols[1], app);
    render_latency_chart(frame, cols[2], app);
}

fn render_download_chart(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let peak_info = app
        .peak_download
        .map(|p| format!(" (Peak: {p:.1} Mbps)"))
        .unwrap_or_default();
    let title = Line::from(vec![
        Span::styled(
            " DOWNLOAD ",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(peak_info, Style::default().fg(MUTED)),
    ]);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_set(symbols::border::PLAIN)
        .border_style(Style::default().fg(PANEL_BORDER))
        .style(Style::default().bg(PANEL));

    if app.download_points.is_empty() {
        let msg = if app.running {
            "Waiting for download measurements..."
        } else {
            "Waiting for test start..."
        };
        let placeholder = Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(MUTED))))
            .block(block)
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(placeholder, area);
        return;
    }

    let max_time = app
        .download_points
        .iter()
        .map(|(t, _)| *t)
        .fold(0.0f64, |acc, t| acc.max(t))
        .max(5.0);

    let max_speed = app
        .download_points
        .iter()
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

    let datasets = vec![Dataset::default()
        .name("Download")
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(CYAN))
        .data(&app.download_points)];

    let chart = Chart::new(datasets)
        .block(block)
        .style(Style::default().bg(PANEL))
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

fn render_upload_chart(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let peak_info = app
        .peak_upload
        .map(|p| format!(" (Peak: {p:.1} Mbps)"))
        .unwrap_or_default();
    let title = Line::from(vec![
        Span::styled(
            " UPLOAD ",
            Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(peak_info, Style::default().fg(MUTED)),
    ]);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_set(symbols::border::PLAIN)
        .border_style(Style::default().fg(PANEL_BORDER))
        .style(Style::default().bg(PANEL));

    if app.upload_points.is_empty() {
        let msg = if app.running {
            "Waiting for upload measurements..."
        } else {
            "Waiting for test start..."
        };
        let placeholder = Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(MUTED))))
            .block(block)
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(placeholder, area);
        return;
    }

    let max_time = app
        .upload_points
        .iter()
        .map(|(t, _)| *t)
        .fold(0.0f64, |acc, t| acc.max(t))
        .max(5.0);

    let max_speed = app
        .upload_points
        .iter()
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

    let datasets = vec![Dataset::default()
        .name("Upload")
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(PURPLE))
        .data(&app.upload_points)];

    let chart = Chart::new(datasets)
        .block(block)
        .style(Style::default().bg(PANEL))
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

    let stats_info = match (app.min_latency, app.max_latency) {
        (Some(min), Some(max)) => format!(" (Min: {min:.1} / Max: {max:.1} ms)"),
        _ => String::new(),
    };

    let title = Line::from(vec![
        Span::styled(
            " LATENCY ",
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        ),
        Span::styled(stats_info, Style::default().fg(MUTED)),
    ]);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_set(symbols::border::PLAIN)
        .border_style(Style::default().fg(PANEL_BORDER))
        .style(Style::default().bg(PANEL));

    if datasets.is_empty() {
        let msg = if app.running {
            "Probing ping latency..."
        } else {
            "Waiting for test start..."
        };
        let placeholder = Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(MUTED))))
            .block(block)
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(placeholder, area);
        return;
    }

    let chart = Chart::new(datasets)
        .block(block)
        .style(Style::default().bg(PANEL))
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

fn render_results(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(symbols::border::PLAIN)
        .border_style(Style::default().fg(PANEL_BORDER))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // RESULTS
    let mut results_lines = vec![Line::from(Span::styled(
        "RESULTS",
        Style::default().fg(INK).add_modifier(Modifier::BOLD),
    ))];
    if let Some(result) = &app.result {
        let dl_str = format!("{:.2}", result.download_mbps);
        let ul_str = format!("{:.2}", result.upload_mbps);
        let lat_str = format!("{:.2}", result.latency_ms);
        let dl_peak = app
            .peak_download
            .map(|p| format!(" (Peak: {p:.2} Mbps)"))
            .unwrap_or_default();
        let ul_peak = app
            .peak_upload
            .map(|p| format!(" (Peak: {p:.2} Mbps)"))
            .unwrap_or_default();
        let lat_stats = match (app.min_latency, app.max_latency) {
            (Some(min), Some(max)) => format!(" (Min: {min:.1} / Max: {max:.1} ms)"),
            _ => String::new(),
        };
        results_lines.push(Line::from(vec![
            Span::styled("  Download: ", Style::default().fg(MUTED)),
            Span::styled(
                dl_str,
                Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Mbps", Style::default().fg(MUTED)),
            Span::styled(dl_peak, Style::default().fg(MUTED)),
        ]));
        results_lines.push(Line::from(vec![
            Span::styled("  Upload:   ", Style::default().fg(MUTED)),
            Span::styled(
                ul_str,
                Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Mbps", Style::default().fg(MUTED)),
            Span::styled(ul_peak, Style::default().fg(MUTED)),
        ]));
        results_lines.push(Line::from(vec![
            Span::styled("  Latency:  ", Style::default().fg(MUTED)),
            Span::styled(
                lat_str,
                Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ms", Style::default().fg(MUTED)),
            Span::styled(lat_stats, Style::default().fg(MUTED)),
        ]));
        results_lines.push(Line::from(vec![
            Span::styled("  Provider: ", Style::default().fg(MUTED)),
            Span::styled("Powered by fast.com", Style::default().fg(INK)),
        ]));
    } else if app.running {
        results_lines.push(Line::from(Span::styled(
            "  Measurement in progress...",
            Style::default().fg(MUTED),
        )));
        results_lines.push(Line::from(vec![
            Span::styled("  Active Phase: ", Style::default().fg(MUTED)),
            Span::styled(
                phase_name(app.phase),
                Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
            ),
        ]));
        results_lines.push(Line::from(vec![
            Span::styled("  Provider: ", Style::default().fg(MUTED)),
            Span::styled("Powered by fast.com", Style::default().fg(INK)),
        ]));
    } else if let Some(err) = &app.error {
        results_lines.push(Line::from(Span::styled(
            format!("  Error: {err}"),
            Style::default().fg(RED),
        )));
        results_lines.push(Line::from(vec![
            Span::styled("  Provider: ", Style::default().fg(MUTED)),
            Span::styled("Powered by fast.com", Style::default().fg(INK)),
        ]));
    } else {
        results_lines.push(Line::from(Span::styled(
            "  No test run recorded",
            Style::default().fg(MUTED),
        )));
        results_lines.push(Line::from(Span::styled(
            "  Press s or Enter to begin speed test",
            Style::default().fg(MUTED),
        )));
        results_lines.push(Line::from(vec![
            Span::styled("  Provider: ", Style::default().fg(MUTED)),
            Span::styled("Powered by fast.com", Style::default().fg(INK)),
        ]));
    }
    frame.render_widget(Paragraph::new(results_lines), inner);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let status_span = if app.phase == TestPhase::Complete {
        Span::styled(
            "Testing completed successfully",
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        )
    } else if app.running {
        Span::styled(
            "Testing in progress...",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        )
    } else if app.error.is_some() {
        Span::styled(
            "Measurement failed · press s to retry",
            Style::default().fg(RED).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "Ready · press s to start a measurement",
            Style::default().fg(MUTED),
        )
    };

    let action = if app.running {
        "test in progress..."
    } else if app.result.is_some() || matches!(app.phase, TestPhase::Complete | TestPhase::Failed) {
        "s / Enter restart"
    } else {
        "s / Enter start"
    };

    let footer = Paragraph::new(Line::from(vec![
        status_span,
        Span::styled("      ", Style::default()),
        Span::styled("[ ", Style::default().fg(PANEL_BORDER)),
        Span::styled(
            action,
            Style::default().fg(INK).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ]", Style::default().fg(PANEL_BORDER)),
        Span::styled("      ", Style::default()),
        Span::styled("[ q / Esc quit ]", Style::default().fg(MUTED)),
    ]))
    .alignment(ratatui::layout::Alignment::Center)
    .style(Style::default().bg(BG));
    frame.render_widget(footer, area);
}

fn render_intermediate(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let show_chart = area.height >= 18;
    let hero_height = if area.height >= 24 { 7 } else { 5 };

    let mut constraints = vec![
        Constraint::Length(1),           // Header
        Constraint::Length(1),           // Divider
        Constraint::Length(hero_height), // Hero
        Constraint::Length(3),           // Metric Triplet
    ];
    if show_chart {
        constraints.push(Constraint::Min(6)); // Active chart
    }
    constraints.push(Constraint::Length(1)); // Footer

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    render_header(frame, sections[0], app);
    render_divider(frame, sections[1]);
    render_hero(frame, sections[2], app);
    render_metric_triplet(frame, sections[3], app);

    if show_chart && sections.len() > 5 {
        match app.phase {
            TestPhase::Upload => render_upload_chart(frame, sections[4], app),
            TestPhase::Latency => render_latency_chart(frame, sections[4], app),
            _ => render_download_chart(frame, sections[4], app),
        }
        render_footer(frame, sections[5], app);
    } else {
        render_footer(frame, sections[4], app);
    }
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
                Span::styled(
                    "↑ ",
                    Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
                ),
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
            (156, 47),
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

    #[test]
    fn test_reference_dashboard_156x47_labels_and_structure() {
        let mut app = App::new();
        app.phase = TestPhase::Complete;
        app.running = false;
        app.download_mbps = Some(73.25);
        app.upload_mbps = Some(26.75);
        app.latency_ms = Some(9.50);
        app.peak_download = Some(75.00);
        app.peak_upload = Some(28.00);
        app.min_latency = Some(8.50);
        app.max_latency = Some(11.00);
        app.avg_latency = Some(9.50);
        app.result = Some(TestResult {
            download_mbps: 73.25,
            upload_mbps: 26.75,
            latency_ms: 9.50,
        });
        app.download_points = vec![(0.0, 50.0), (1.0, 73.25)];
        app.upload_points = vec![(1.0, 20.0), (2.0, 26.75)];
        app.latency_points = vec![(1.0, 9.50)];
        app.progress = 1.0;

        let backend = TestBackend::new(156, 47);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| ui(f, &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();

        let mut rendered = String::new();
        for y in 0..buffer.area.height {
            let mut line = String::new();
            for x in 0..buffer.area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            rendered.push_str(line.trim_end());
            rendered.push('\n');
        }

        assert!(rendered.contains("FAST/TEST"));
        assert!(rendered
            .to_ascii_lowercase()
            .contains("internet speed test"));
        assert!(rendered
            .to_ascii_lowercase()
            .contains("your internet speed"));
        assert!(rendered.contains("Complete"));
        assert!(rendered.contains("100%"));
        assert!(rendered.contains("DOWNLOAD"));
        assert!(rendered.contains("UPLOAD"));
        assert!(rendered.contains("LATENCY"));
        assert!(rendered.contains("RESULTS"));
        assert!(rendered.contains("Powered by fast.com"));
        assert!(rendered.contains("Testing completed successfully"));
        assert!(rendered.contains("73.25"));
        assert!(rendered.contains("26.75"));
        assert!(rendered.contains("9.50"));
    }
}
