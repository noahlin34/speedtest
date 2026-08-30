# Fast.com terminal speed test

This project is a small Ratatui frontend for measuring a connection against Fast.com infrastructure from a terminal. It keeps the terminal interactive while the network test runs and displays the live phase, progress, throughput, and latency reported by the backend.

## Build and run

A current stable Rust toolchain is required.

```sh
cargo build --release
cargo run --release
```

The program uses a TLS-enabled HTTP client and needs network access to Fast.com. Press `q` or `Esc` to leave the interface.

## Controls

- `s` or `Enter`: start a test when idle, or start another test after the previous one has finished.
- `q` or `Esc`: quit.

Only one test can run at a time. While a test is active, additional start keys are ignored.

## Fast.com requests

The backend first contacts Fast.com's speed-test API at
`https://api.fast.com/netflix/speedtest/v2` to obtain the token/target metadata and the download URLs selected by Fast.com. It streams data from multiple returned download targets concurrently and derives download throughput from the bytes and elapsed time.

When the target response advertises upload measurement endpoints, the backend generates request bytes and POSTs them to those endpoints. If upload measurement is unavailable or fails, the interface reports that failure instead of displaying a fabricated upload value. Latency is measured using the available Fast.com targets and is reported separately from throughput.

This is an independent client using Fast.com's public service, not the Fast.com website itself. Results can differ from the website because of target selection, request scheduling, TLS/HTTP behavior, local CPU and network conditions, browser optimizations, and the timing window used by this client. The values should therefore be treated as an informative measurement rather than an exact reproduction of the website's result.
