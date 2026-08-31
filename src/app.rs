use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
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

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(inner);

    render_header(frame, sections[0], app);
    render_hero(frame, sections[1], app);
    render_secondary_cards(frame, sections[2], app);
    render_footer(frame, sections[3], app);
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

    // Split hero into Title, Digits, Subtitle, Progress Bar, Stepper
    let total_content_height = 8;
    let (top_pad, bot_pad) = if inner.height > total_content_height {
        let rem = inner.height - total_content_height;
        (rem / 2, rem - rem / 2)
    } else {
        (0, 0)
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_pad), // Dynamic top centering pad
            Constraint::Length(1),       // Title
            Constraint::Length(3),       // 3-line Big Digits
            Constraint::Length(1),       // Subtitle
            Constraint::Length(1),       // Spacing
            Constraint::Length(1),       // Progress bar
            Constraint::Length(1),       // Pipeline Stepper
            Constraint::Length(bot_pad), // Dynamic bottom pad
        ])
        .split(inner);

    // 1. Title
    let title_line = Paragraph::new(Line::from(Span::styled(
        hero_title,
        Style::default()
            .fg(if app.running { hero_color } else { MUTED })
            .add_modifier(Modifier::BOLD),
    )))
    .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(title_line, rows[1]);

    // 2. Big Digits
    if rows[2].height >= 3 && rows[2].width >= 20 {
        let big_lines = build_big_number_lines(&val_str, unit_str, hero_color);
        let digit_para = Paragraph::new(big_lines).alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(digit_para, rows[2]);
    } else {
        let fallback_line = Paragraph::new(Line::from(vec![
            Span::styled(
                &val_str,
                Style::default().fg(hero_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {unit_str}"), Style::default().fg(MUTED)),
        ]))
        .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(fallback_line, rows[2]);
    }

    // 3. Subtitle
    let sub_line = Paragraph::new(Line::from(Span::styled(
        subtitle,
        Style::default().fg(if app.error.is_some() { RED } else { MUTED }),
    )))
    .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(sub_line, rows[3]);
    // 4. Progress Bar
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
    frame.render_widget(gauge, rows[5]);
    render_stepper(frame, rows[6], app);
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

    render_metric_card(
        frame,
        cards[0],
        " ⬇ DOWNLOAD ",
        app.download_mbps,
        "Mbps",
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

    let content = Paragraph::new(Line::from(vec![
        Span::styled(
            number,
            Style::default()
                .fg(value_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {unit}"), Style::default().fg(MUTED)),
        badge,
    ]))
    .alignment(ratatui::layout::Alignment::Center);

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
        app.progress = 0.45;
        app.status = "Measuring download throughput".to_string();

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
