use crate::app::{ui, App};
use crate::speed::{SpeedUpdate, TestPhase, TestResult};
use ratatui::{backend::TestBackend, Terminal};

fn completed_app() -> App {
    let mut app = App::new();
    app.reset();
    app.set_running(true);

    // Feed the same public events the orchestration layer emits so the rendered
    // result includes real telemetry points as well as final measurements.
    app.apply_update(SpeedUpdate::Phase(TestPhase::FetchingTargets));
    app.apply_update(SpeedUpdate::Phase(TestPhase::Latency));
    app.apply_update(SpeedUpdate::LatencyMs(10.25));
    app.apply_update(SpeedUpdate::LatencyMs(9.50));
    app.apply_update(SpeedUpdate::Phase(TestPhase::Download));
    app.apply_update(SpeedUpdate::DownloadMbps(68.75));
    app.apply_update(SpeedUpdate::DownloadMbps(73.25));
    app.apply_update(SpeedUpdate::Phase(TestPhase::Upload));
    app.apply_update(SpeedUpdate::UploadMbps(24.50));
    app.apply_update(SpeedUpdate::UploadMbps(26.75));
    app.apply_update(SpeedUpdate::Progress(0.99));
    app.apply_update(SpeedUpdate::Complete(TestResult {
        download_mbps: 73.25,
        upload_mbps: 26.75,
        latency_ms: 9.50,
    }));
    app
}

fn rendered_text(app: &App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("TestBackend should construct");
    terminal
        .draw(|frame| ui(frame, app))
        .expect("dashboard should render to TestBackend");

    let buffer = terminal.backend().buffer();
    let mut rows = Vec::with_capacity(buffer.area.height as usize);
    for y in 0..buffer.area.height {
        let mut row = String::with_capacity(buffer.area.width as usize);
        for x in 0..buffer.area.width {
            row.push_str(buffer[(x, y)].symbol());
        }
        rows.push(row);
    }
    rows.join("\n")
}

fn assert_contains(text: &str, expected: &str) {
    assert!(
        text.to_ascii_lowercase()
            .contains(&expected.to_ascii_lowercase()),
        "expected rendered dashboard to contain {expected:?}; rendered text was:\n{text}"
    );
}

#[test]
fn completed_dashboard_matches_reference_regions() {
    let app = completed_app();
    let text = rendered_text(&app, 156, 47);

    // Header branding, title, and actionable controls.
    assert!(
        text.contains("FAST/TEST"),
        "header branding disappeared:\n{text}"
    );
    assert_contains(&text, "Internet Speed Test");
    assert!(
        text.contains("Press 's' or Enter to test again · 'q' to quit"),
        "exact completed-state header controls disappeared:\n{text}"
    );

    // Hero completion state and progress indicator.
    assert_contains(&text, "Your Internet Speed");
    assert_contains(&text, "Complete");
    assert_contains(&text, "100%");

    // Every telemetry panel keeps its own visible heading. Checking each token
    // independently avoids coupling this contract to panel orientation or box
    // glyph coordinates while still catching a collapsed/missing chart.
    for heading in ["DOWNLOAD", "UPLOAD", "LATENCY"] {
        assert_contains(&text, heading);
    }

    // Lower reference panels and provider/success copy.
    assert_contains(&text, "RESULTS");
    assert_contains(&text, "ABOUT");
    assert_contains(&text, "Powered by fast.com");
    assert_contains(&text, "Testing completed successfully");

    // Final values must remain visible in the rendered result, not just in App.
    for metric in ["73.25", "26.75", "9.50"] {
        assert_contains(&text, metric);
    }
    assert_contains(&text, "Mbps");
    assert_contains(&text, "ms");
}

#[test]
fn compact_rendering_smoke_covers_boundary_states() {
    let mut idle = App::new();
    idle.reset();

    let mut running = App::new();
    running.reset();
    running.set_running(true);
    running.apply_update(SpeedUpdate::Phase(TestPhase::FetchingTargets));

    let mut failed = App::new();
    failed.reset();
    failed.apply_update(SpeedUpdate::Failed("connection unavailable".to_string()));

    let completed = completed_app();
    let states = [&idle, &running, &failed, &completed];
    for (width, height) in [(1, 1), (20, 5), (53, 13)] {
        for app in states {
            // The absence of a panic is the compact-layout contract. Keep the
            // output materialized too, ensuring the backend actually completed a
            // draw rather than merely constructing a Terminal.
            let output = rendered_text(app, width, height);
            assert_eq!(output.lines().count(), height as usize);
        }
    }
}
