mod app;
mod speed;
#[cfg(test)]
mod ui_reference_tests;

use std::{
    io::{self, Stdout},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;

use app::App;
use speed::{SpeedUpdate, TestResult};

type CrosstermTerminal = Terminal<CrosstermBackend<Stdout>>;

/// Owns the terminal state changed by the application and restores it on every
/// return path, including errors while entering the alternate screen.
struct TerminalGuard {
    terminal: CrosstermTerminal,
    needs_restore: bool,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))
            .context("creating the terminal renderer")?;
        let mut guard = Self {
            terminal,
            needs_restore: false,
        };

        enable_raw_mode().context("enabling terminal raw mode")?;
        guard.needs_restore = true;
        execute!(guard.terminal.backend_mut(), EnterAlternateScreen)
            .context("entering the alternate screen")?;
        guard
            .terminal
            .clear()
            .context("clearing the terminal screen")?;
        Ok(guard)
    }

    fn restore(&mut self) -> Result<()> {
        if !self.needs_restore {
            return Ok(());
        }
        // Mark this before attempting cleanup so Drop does not issue a second
        // sequence if one of the cleanup operations fails.
        self.needs_restore = false;

        let mut first_error = None;
        if let Err(error) = disable_raw_mode() {
            first_error = Some(anyhow!(error).context("disabling terminal raw mode"));
        }
        if let Err(error) = execute!(self.terminal.backend_mut(), LeaveAlternateScreen) {
            if first_error.is_none() {
                first_error = Some(anyhow!(error).context("leaving the alternate screen"));
            }
        }
        if let Err(error) = self.terminal.show_cursor() {
            if first_error.is_none() {
                first_error = Some(anyhow!(error).context("restoring the terminal cursor"));
            }
        }

        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

/// Reads crossterm events away from the async loop so a pending terminal read
/// never prevents speed updates from being rendered.
struct EventPump {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl EventPump {
    fn start(events_tx: mpsc::UnboundedSender<Event>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let join = thread::spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                match event::poll(Duration::from_millis(100)) {
                    Ok(true) => match event::read() {
                        Ok(event) => {
                            if events_tx.send(event).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    },
                    Ok(false) => {}
                    Err(_) => break,
                }
            }
        });

        Self {
            stop,
            join: Some(join),
        }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for EventPump {
    fn drop(&mut self) {
        self.stop();
    }
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("speedtest: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let mut terminal = TerminalGuard::enter().context("initializing the terminal UI")?;
    let mut app = App::new();

    let app_result = run_app(&mut terminal.terminal, &mut app).await;
    let restore_result = terminal.restore();

    match (app_result, restore_result) {
        (Err(error), Err(restore_error)) => Err(anyhow!(
            "{error:#}; terminal restoration also failed: {restore_error:#}"
        )),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(anyhow!("restoring the terminal: {error:#}")),
        (Ok(()), Ok(())) => Ok(()),
    }
}

async fn run_app(terminal: &mut CrosstermTerminal, app: &mut App) -> Result<()> {
    let (events_tx, mut events_rx) = mpsc::unbounded_channel::<Event>();
    let mut event_pump = EventPump::start(events_tx);
    let (updates_tx, mut updates_rx) = mpsc::unbounded_channel::<SpeedUpdate>();
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<
        std::result::Result<anyhow::Result<TestResult>, tokio::task::JoinError>,
    >();
    let mut test_active = false;
    let mut terminal_update_seen = false;
    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let initial_draw = terminal
        .draw(|frame| app::ui(frame, app))
        .context("drawing the initial terminal frame");
    if let Err(error) = initial_draw {
        event_pump.stop();
        return Err(error);
    }

    let result: Result<()> = async {
        loop {
            tokio::select! {
                maybe_event = events_rx.recv() => {
                    let Some(event) = maybe_event else {
                        anyhow::bail!("terminal event reader stopped unexpectedly");
                    };
                    let Event::Key(key) = event else {
                        continue;
                    };
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char('s') | KeyCode::Enter if !app.is_running() && !test_active => {
                            app.reset();
                            app.set_running(true);
                            terminal_update_seen = false;
                            test_active = true;

                            let worker_updates = updates_tx.clone();
                            let worker = tokio::spawn(async move {
                                speed::run_speed_test(worker_updates).await
                            });
                            let done_sender = done_tx.clone();
                            tokio::spawn(async move {
                                let completion = worker.await;
                                let _ = done_sender.send(completion);
                            });
                        }
                        _ => {}
                    }
                }
                maybe_update = updates_rx.recv() => {
                    let Some(update) = maybe_update else {
                        anyhow::bail!("speed-test update channel closed unexpectedly");
                    };
                    let terminal_update = matches!(
                        &update,
                        SpeedUpdate::Complete(_) | SpeedUpdate::Failed(_)
                    );
                    app.apply_update(update);
                    if terminal_update {
                        terminal_update_seen = true;
                        app.set_running(false);
                    }
                }
                maybe_done = done_rx.recv() => {
                    let Some(completion) = maybe_done else {
                        anyhow::bail!("speed-test completion channel closed unexpectedly");
                    };
                    test_active = false;
                    match completion {
                        Ok(Ok(_result)) => {}
                        Ok(Err(error)) if !terminal_update_seen => {
                            app.apply_update(SpeedUpdate::Failed(format!("{error:#}")));
                        }
                        Err(error) if !terminal_update_seen => {
                            app.apply_update(SpeedUpdate::Failed(format!(
                                "speed-test task failed: {error}"
                            )));
                        }
                        _ => {}
                    }
                    app.set_running(false);
                }
                _ = ticker.tick() => {
                    terminal
                        .draw(|frame| app::ui(frame, app))
                        .context("drawing the terminal frame")?;
                }
            }
        }
        Ok(())
    }
    .await;

    event_pump.stop();
    result
}
