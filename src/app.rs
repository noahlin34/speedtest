use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Frame,
};

use crate::speed::{SpeedUpdate, TestPhase, TestResult};

const BG: Color = Color::Rgb(9, 15, 25);
const PANEL: Color = Color::Rgb(16, 26, 40);
const PANEL_ALT: Color = Color::Rgb(21, 34, 51);
const INK: Color = Color::Rgb(227, 238, 249);
const MUTED: Color = Color::Rgb(137, 160, 183);
const CYAN: Color = Color::Rgb(79, 216, 218);
const BLUE: Color = Color::Rgb(91, 145, 255);
const AMBER: Color = Color::Rgb(246, 185, 76);
const RED: Color = Color::Rgb(246, 104, 112);
const GREEN: Color = Color::Rgb(105, 218, 149);

/// State held by the terminal dashboard while a Fast.com measurement runs.
#[derive(Debug)]
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
                        self.status = phase_status(phase).to_string();
                    }
                    TestPhase::Download | TestPhase::Upload | TestPhase::Latency => {
                        self.running = true;
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
                    if self.phase == TestPhase::Download {
                        self.status = "Measuring download throughput".to_string();
                    }
                }
            }
            SpeedUpdate::UploadMbps(value) => {
                if let Some(value) = valid_measurement(value) {
                    self.upload_mbps = Some(value);
                    if self.phase == TestPhase::Upload {
                        self.status = "Measuring upload throughput".to_string();
                    }
                }
            }
            SpeedUpdate::LatencyMs(value) => {
                if let Some(value) = valid_measurement(value) {
                    self.latency_ms = Some(value);
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
            Span::styled("· network instrument", Style::default().fg(MUTED)),
        ]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(47, 76, 105)))
        .style(Style::default().bg(BG));
    let inner = shell.inner(area);
    frame.render_widget(shell, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(7),
            Constraint::Length(3),
        ])
        .split(inner);
    render_header(frame, sections[0], app);

    let body = if sections[1].width >= 92 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(sections[1])
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(8), Constraint::Min(6)])
            .split(sections[1])
    };
    render_metrics(frame, body[0], app);
    render_progress(frame, body[1], app);
    render_footer(frame, sections[2], app);
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let phase = phase_name(app.phase);
    let activity = if app.running {
        "LIVE MEASUREMENT"
    } else {
        "STANDBY"
    };
    let activity_color = if app.running { GREEN } else { MUTED };
    let header = Paragraph::new(vec![
        Line::from(Span::styled(
            "Connection profile",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                activity,
                Style::default()
                    .fg(activity_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("   ", Style::default()),
            Span::styled(phase, Style::default().fg(INK)),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(PANEL_ALT)),
    );
    frame.render_widget(header, area);
}

fn render_metrics(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title(Span::styled(
            " MEASUREMENTS ",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PANEL_ALT))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let cards = if inner.width >= 60 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(34),
                Constraint::Percentage(33),
                Constraint::Percentage(33),
            ])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Length(2),
            ])
            .split(inner)
    };

    render_metric(frame, cards[0], "DOWNLOAD", app.download_mbps, "Mbps", BLUE);
    render_metric(frame, cards[1], "UPLOAD", app.upload_mbps, "Mbps", CYAN);
    render_metric(frame, cards[2], "LATENCY", app.latency_ms, "ms", AMBER);
}

fn render_metric(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: Option<f64>,
    unit: &str,
    color: Color,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let number = match value {
        Some(value) => format_measurement(value),
        None => "—".to_string(),
    };
    let lines = vec![
        Line::from(Span::styled(
            label,
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                number,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {unit}"), Style::default().fg(MUTED)),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(PANEL)),
        area,
    );
}

fn render_progress(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title(Span::styled(
            " TEST SIGNAL ",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PANEL_ALT))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(2),
        ])
        .split(inner);
    let status_color = if app.error.is_some() { RED } else { INK };
    frame.render_widget(
        Paragraph::new(app.error.as_deref().unwrap_or(&app.status))
            .style(Style::default().fg(status_color))
            .wrap(Wrap { trim: true }),
        rows[0],
    );

    let gauge_label = format!("{}  {:>3.0}%", phase_name(app.phase), app.progress * 100.0);
    let gauge_color = if app.error.is_some() {
        RED
    } else if app.phase == TestPhase::Complete {
        GREEN
    } else {
        CYAN
    };
    frame.render_widget(
        Gauge::default()
            .block(Block::default().style(Style::default().bg(PANEL_ALT)))
            .gauge_style(Style::default().fg(gauge_color).bg(Color::Rgb(30, 48, 69)))
            .percent((app.progress * 100.0).round().clamp(0.0, 100.0) as u16)
            .label(gauge_label),
        rows[1],
    );

    let next = match app.phase {
        TestPhase::Idle => "Press s or Enter to start a measurement",
        TestPhase::Complete => "Press s or Enter to run another measurement",
        TestPhase::Failed => "Press s or Enter to retry · q to quit",
        _ => "Keep this pane open while the signal settles",
    };
    frame.render_widget(
        Paragraph::new(next)
            .style(Style::default().fg(MUTED))
            .wrap(Wrap { trim: true }),
        rows[2],
    );
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let action = if app.running {
        "test running"
    } else if app.result.is_some() || app.phase == TestPhase::Failed {
        "s/Enter restart"
    } else {
        "s/Enter start"
    };
    let footer = Paragraph::new(Line::from(vec![
        Span::styled("[", Style::default().fg(AMBER)),
        Span::styled(
            action,
            Style::default().fg(INK).add_modifier(Modifier::BOLD),
        ),
        Span::styled("]", Style::default().fg(AMBER)),
        Span::styled("    ", Style::default()),
        Span::styled("[q quit]", Style::default().fg(MUTED)),
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
        Span::styled("  ·  ", Style::default().fg(MUTED)),
        Span::styled(phase_name(app.phase), Style::default().fg(AMBER)),
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
                Span::styled("↓ ", Style::default().fg(BLUE)),
                Span::styled(
                    format_measurement_or_dash(app.download_mbps),
                    Style::default().fg(INK),
                ),
                Span::styled(" Mbps", Style::default().fg(MUTED)),
                Span::styled("   ↑ ", Style::default().fg(CYAN)),
                Span::styled(
                    format_measurement_or_dash(app.upload_mbps),
                    Style::default().fg(INK),
                ),
                Span::styled(" Mbps", Style::default().fg(MUTED)),
            ]),
            Line::from(vec![
                Span::styled("◌ ", Style::default().fg(AMBER)),
                Span::styled(
                    format_measurement_or_dash(app.latency_ms),
                    Style::default().fg(INK),
                ),
                Span::styled(" ms", Style::default().fg(MUTED)),
                Span::styled("   ", Style::default()),
                Span::styled(
                    app.error.as_deref().unwrap_or(&app.status),
                    Style::default().fg(if app.error.is_some() { RED } else { MUTED }),
                ),
            ]),
            Line::from(Span::styled(
                format!("{}  {:>3.0}%", phase_name(app.phase), app.progress * 100.0),
                Style::default().fg(CYAN),
            )),
        ];
        frame.render_widget(
            Paragraph::new(content)
                .wrap(Wrap { trim: true })
                .style(Style::default().bg(PANEL)),
            rows[1],
        );
    }

    let action = if app.running {
        "test running"
    } else if app.result.is_some() || app.phase == TestPhase::Failed {
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

fn phase_name(phase: TestPhase) -> &'static str {
    match phase {
        TestPhase::Idle => "Ready",
        TestPhase::FetchingTargets => "Finding test targets",
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
        TestPhase::Complete => "Measurement complete · press r to run again",
        TestPhase::Failed => "Measurement failed · press r to retry",
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
