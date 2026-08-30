# Repository Guidelines

## Project Overview
This is a single Rust 2021 binary package (`speedtest` 0.1.0) providing a Ratatui terminal frontend for Fast.com connection measurements. The UI reports phase, progress, download/upload throughput, and latency. A stable Rust toolchain and network access to Fast.com are required. Upload failures are reported; they must not be replaced with fabricated values.

## Architecture & Data Flow
- `src/main.rs::run` owns terminal lifecycle: it enters `TerminalGuard`, creates `App`, runs `run_app`, then restores the terminal and combines application/restoration errors.
- `EventPump::start` performs blocking `crossterm::event::poll/read` on a dedicated thread and sends `Event` values through a Tokio unbounded channel. `EventPump::stop` uses its `Arc<AtomicBool>` stop flag and joins the thread; do not move blocking input reads into the async loop.
- `run_app` creates separate unbounded channels for input, `SpeedUpdate`, and worker completion. It draws once initially, then `tokio::select!` handles key events, backend updates, worker completion, and a 100 ms draw ticker. `q`/Esc quits; `s`/Enter starts only when idle or complete, while `test_active` independently prevents concurrent starts.
- A start resets the app, marks it running, clears completion bookkeeping, spawns `speed::run_speed_test`, and forwards its `JoinHandle` result to the completion channel. `App::apply_update` consumes updates in arrival order and drives the rendered state.
- Backend discovery starts with `GET https://fast.com/` to find an inline token or `app-*.js` bundle token. The token is then sent to `https://api.fast.com/netflix/speedtest/v2` (`https=true`, `urlCount=5`); this v2 API returns the measurement targets. The backend measures latency from the first target, streams concurrent downloads (up to four jobs), then POSTs 8 MiB payloads concurrently (up to four jobs) for uploads.
- Backend updates establish `TestPhase`, progress, and measurements; successful completion emits `Phase(Complete)`, `Progress(1.0)`, then `Complete(TestResult)`. Failure emits `Phase(Failed)` then `Failed(String)`. Completion handling converts inner/task failures to a failure update only when a terminal backend update was not already seen, avoiding duplicate failure UI.
- Cross-module contracts matter: `SpeedUpdate`, `TestPhase`, and `TestResult` field names/types are consumed directly by `App::apply_update`; `App`'s public fields are renderer-facing. `reset()` must precede a new run, and `FetchingTargets` defensively clears stale metrics. Discovery must yield at least one valid HTTP(S) target; download/upload accept partial target success but fail when all requests fail. Measurements must be finite and nonnegative; progress is finite and clamped to `[0, 1]`.

## Key Directories
- `src/`: all application code, split between terminal orchestration (`main.rs`), view model/rendering (`app.rs`), and HTTP measurement (`speed.rs`).
- `target/`: Cargo build output; ignored by `.gitignore` and not a source or hand-edit location.
- Repository root: `Cargo.toml`, `Cargo.lock`, and `README.md` define package/dependency resolution and user-facing controls/commands. There are no configured scripts or task-runner commands evidenced by the project configuration.

## Development Commands
Use Cargo from the repository root:

```text
cargo check                 # fast compile/type-check
cargo build                 # debug build
cargo build --release       # documented release build
cargo run                   # interactive TUI during development
cargo run --release         # documented release-like run
cargo test                  # all built-in tests
cargo test speed::tests::extracts_token_from_javascript_and_json
cargo test -- --nocapture   # preserve test output when needed
cargo fmt -- --check        # formatting check
cargo clippy --all-targets --all-features -- -D warnings
```

`cargo check --locked` (and the corresponding build/test commands with `--locked`) is appropriate when requiring the checked-in `Cargo.lock`; the lockfile is generated and should not be edited manually. No repository-specific CI gate, script, alias, or test runner is declared, so treat the commands above as useful Cargo workflows rather than hidden project policy.

## Code Conventions & Common Patterns
- Follow Rust 2021 conventions: four-space indentation; `snake_case` functions/locals; `CamelCase` types/enums; `SCREAMING_SNAKE_CASE` constants; imports grouped by standard library, external crates, then crate modules.
- Prefer `anyhow::{Context, Result}`, `?`, `bail!`, and operation-specific context strings for errors. Validate HTTP responses through `ensure_success` before reading bodies; skip malformed target records, but fail if no valid targets remain.
- Match `SpeedUpdate` and `TestPhase` exhaustively. Ignore channel sends only where receiver closure is expected (`let _ = updates.send(...)`); an unexpectedly closed orchestration channel is fatal.
- Keep async HTTP/tasks on Tokio; keep blocking crossterm I/O on `EventPump`'s standard thread. Use existing bounded metadata/resource limits: token-page reads up to 2 MiB, target inspection up to 16 records, four measurement workers, and an 8 MiB upload payload. Throughput is decimal Mbps (`bytes * 8 / seconds / 1_000_000`) with zero-duration protection.
- Guard Ratatui layout operations against zero-sized rectangles. `app::ui` switches to compact rendering below width 54 or height 14. Keep helpers private unless a cross-module contract requires otherwise, and document non-obvious invariants.
- Preserve user-visible controls and state semantics: `s`/Enter starts, q/Esc quits, and `App::set_running(false)` must not overwrite terminal status/error phases. Keep `phase_status` text synchronized with actual key bindings (the current observed text mentions `r`, while input accepts `s`/Enter).

## Important Files
- `src/main.rs`: `main`, `run`, `run_app`, `TerminalGuard::{enter,restore}`, `EventPump::{start,stop}`, and the `CrosstermTerminal` alias; owns terminal cleanup, channels, select-loop orchestration, and worker error reconciliation.
- `src/app.rs`: public `App`, `App::{new,apply_update,reset,is_running,set_running}`, and public `ui`; owns state transitions, status/error/result formatting, and responsive Ratatui rendering.
- `src/speed.rs`: public `TestPhase`, `SpeedUpdate`, `TestResult`, and async `run_speed_test`; private token/target discovery, latency/download/upload workers, parsers, validation, and arithmetic helpers.
- `Cargo.toml`: package metadata and direct dependency feature contract. `reqwest` disables defaults and enables `json`, `rustls-tls`, and `stream`; Tokio enables `macros`, `rt-multi-thread`, `sync`, and `time`.
- `Cargo.lock`: generated format-4 resolution with exact versions/checksums; update through Cargo, never by hand. `README.md` documents release commands, controls, Fast.com behavior, and result caveats.

## Runtime/Tooling Preferences
Use a stable Rust toolchain compatible with edition 2021; no MSRV, target, profile, or build-script requirement is declared. Tokio's multithread runtime supplies async tasks, channels, and timing. Use Reqwest's configured Rustls TLS backend rather than reintroducing default TLS features; retain its JSON and streaming capabilities and the 30-second client timeout/user-agent (`speedtest-ratatui/1.0`). Keep network endpoints and Fast.com request sequencing centralized in `src/speed.rs`.

## Testing & QA
Tests use Rust's built-in harness and are colocated in `src/speed.rs`'s `#[cfg(test)] mod tests`, accessing private helpers with `use super::*`. The four existing synchronous tests cover JavaScript/JSON token extraction, identifier/value rejection, `app-*.js` path extraction, and decimal Mbps arithmetic (including zero duration). `src/main.rs` and `src/app.rs` have no observed test modules; the manifest declares no dev-dependencies, integration-test target, async test, coverage, or test-script configuration.

Run `cargo test` (or an exact filter) after backend/helper changes, and use `cargo check` plus the formatting/lint commands when the surrounding change warrants them. Meaningful additions should cover observable contracts not currently exercised: `App` update/reset/running transitions, finite/nonnegative measurement validation and progress clamping, phase/error/result sequencing, worker error propagation, and rendering boundaries. For network or terminal changes, also perform an interactive `cargo run --release` smoke check with Fast.com access: start/restart only from allowed states, observe live updates through completion or reported failure, verify latency/download/upload behavior, and confirm terminal restoration on quit and errors. Avoid claiming deterministic network coverage from the existing unit tests.
