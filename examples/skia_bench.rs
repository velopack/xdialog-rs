//! Skia backend render-pipeline benchmark harness.
//!
//! Drives the continuously-animating indeterminate progress bar to produce a deterministic,
//! input-free render load, then lets `skia-instrumentation`'s `report()` emit the timing stats when
//! the event loop exits. On Linux the default backend is skia, so no override is needed.
//!
//! Env knobs:
//! - `XDIALOG_BENCH_DURATION_SECS` — animation duration (default 10).
//! - `XDIALOG_BENCH_UNCAPPED=1` — run the loop uncapped (max throughput) instead of the 60fps cap.
//!
//! Run with: cargo run --features skia-instrumentation --example skia_bench

use std::time::Duration;

use xdialog::*;

fn main() {
    XDialogBuilder::new().run(run);
}

fn run() {
    let secs: u64 = std::env::var("XDIALOG_BENCH_DURATION_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    // `unwrap()` so a missing display fails the CI step loudly instead of silently reporting nothing.
    let p = show_progress_ex(XDialogOptions {
        title: "Skia Bench".into(),
        main_instruction: "Benchmarking render pipeline".into(),
        message: "Running indeterminate animation...".into(),
        icon: XDialogIcon::Information,
        buttons: vec![],
    })
    .unwrap();
    p.set_indeterminate().unwrap();
    std::thread::sleep(Duration::from_secs(secs));
    p.close().unwrap();
}
