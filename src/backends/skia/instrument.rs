//! Per-frame render/present instrumentation for the skia backend.
//!
//! Entirely feature-gated behind `skia-instrumentation`; compiles out of the shipping crate.
//!
//! The skia event loop, `render_and_present`, and `report()` all run on the same thread (the one
//! that called `run_loop`), so the collector is a `thread_local!` `RefCell<Recorder>` — no locks,
//! and no `&mut` threaded through every call site. Driven by `examples/skia_bench.rs`.

use std::cell::RefCell;
use std::time::{Duration, Instant};

/// One rendered frame: how long the component paint loop took, how long the present took, and when
/// the frame was recorded (used to derive achieved FPS and the dropped-frame count).
struct FrameSample {
    render: Duration,
    present: Duration,
    at: Instant,
}

struct Recorder {
    samples: Vec<FrameSample>,
    /// Whether the loop is running uncapped (max-throughput). Resolved once from the environment.
    uncapped: bool,
}

thread_local! {
    static RECORDER: RefCell<Recorder> = RefCell::new(Recorder {
        samples: Vec::new(),
        uncapped: std::env::var("XDIALOG_BENCH_UNCAPPED").map(|v| v == "1").unwrap_or(false),
    });
}

/// Number of initial frames discarded before computing stats: first pixmap alloc, initial layout,
/// font shaping, and surface warm-up all land here and would skew the percentiles.
const WARMUP_FRAMES: usize = 30;

/// Whether the event loop should run uncapped (`ControlFlow::Poll`) instead of the 60fps cap.
/// Reads `XDIALOG_BENCH_UNCAPPED` (cached on first access).
pub fn uncapped() -> bool {
    RECORDER.with(|r| r.borrow().uncapped)
}

/// Record one rendered frame. `present`-only (expose) frames pass a zero render duration.
pub fn record_frame(render: Duration, present: Duration) {
    let at = Instant::now();
    RECORDER.with(|r| {
        r.borrow_mut().samples.push(FrameSample { render, present, at });
    });
}

/// Compute statistics over the recorded frames and emit them to stderr, the GitHub step summary
/// (if `GITHUB_STEP_SUMMARY` is set), and as a `::notice` annotation.
pub fn report() {
    RECORDER.with(|r| {
        let rec = r.borrow();
        let mode = if rec.uncapped { "uncapped" } else { "capped" };

        let total = rec.samples.len();
        if total <= WARMUP_FRAMES {
            eprintln!(
                "Skia Bench ({mode}): only {total} frames recorded (<= {WARMUP_FRAMES} warm-up); \
                 nothing to report."
            );
            return;
        }

        let samples = &rec.samples[WARMUP_FRAMES..];
        let frames = samples.len();

        let render = Stats::compute(samples.iter().map(|s| s.render));
        let present = Stats::compute(samples.iter().map(|s| s.present));
        // Inter-frame interval (pacing): the gap between consecutive presented frames. Average FPS
        // hides choppiness — a stream that bursts then stalls has a fine mean but a high p95/p99
        // here. This is the metric that actually reflects perceived smoothness.
        let interval = Stats::compute(
            samples.windows(2).map(|w| w[1].at.duration_since(w[0].at)),
        );

        // Achieved FPS over the measured window (first..last recorded frame).
        let elapsed = samples[frames - 1].at.duration_since(samples[0].at);
        let elapsed_secs = elapsed.as_secs_f64();
        let fps = if elapsed_secs > 0.0 { frames as f64 / elapsed_secs } else { 0.0 };

        // Dropped frames only make sense against the 60fps target (capped mode).
        let dropped: Option<i64> = if rec.uncapped {
            None
        } else {
            let expected = (elapsed_secs * 60.0).round() as i64;
            Some((expected - frames as i64).max(0))
        };

        let report = Report {
            mode,
            frames,
            discarded: WARMUP_FRAMES,
            fps,
            render,
            present,
            interval,
            dropped,
        };
        report.emit_stderr();
        report.emit_step_summary();
        report.emit_notice();
    });
}

/// Computed statistics for one benchmark phase, ready to render to each output sink.
struct Report {
    mode: &'static str,
    frames: usize,
    discarded: usize,
    fps: f64,
    render: Stats,
    present: Stats,
    interval: Stats,
    dropped: Option<i64>,
}

/// Summary statistics (milliseconds) for one stage.
struct Stats {
    mean: f64,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
}

impl Stats {
    fn compute(durs: impl Iterator<Item = Duration>) -> Self {
        let mut ms: Vec<f64> = durs.map(|d| d.as_secs_f64() * 1000.0).collect();
        if ms.is_empty() {
            return Self { mean: 0.0, p50: 0.0, p95: 0.0, p99: 0.0, max: 0.0 };
        }
        let mean = ms.iter().sum::<f64>() / ms.len() as f64;
        ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let pct = |p: f64| {
            let idx = ((p / 100.0) * (ms.len() - 1) as f64).round() as usize;
            ms[idx.min(ms.len() - 1)]
        };
        Self {
            mean,
            p50: pct(50.0),
            p95: pct(95.0),
            p99: pct(99.0),
            max: *ms.last().unwrap(),
        }
    }
}

fn dropped_str(dropped: Option<i64>) -> String {
    dropped.map(|d| d.to_string()).unwrap_or_else(|| "N/A".to_string())
}

impl Report {
    fn emit_stderr(&self) {
        let Report { mode, frames, discarded, fps, render, present, interval, dropped } = self;
        eprintln!();
        eprintln!("─── Skia Bench ({mode}) ───");
        eprintln!("frames: {frames} (discarded {discarded} warm-up)   achieved FPS: {fps:.1}   dropped: {}", dropped_str(*dropped));
        eprintln!("stage      mean      p50      p95      p99      max   (ms)");
        eprintln!(
            "render   {:>6.3}  {:>6.3}  {:>6.3}  {:>6.3}  {:>6.3}",
            render.mean, render.p50, render.p95, render.p99, render.max
        );
        eprintln!(
            "present  {:>6.3}  {:>6.3}  {:>6.3}  {:>6.3}  {:>6.3}",
            present.mean, present.p50, present.p95, present.p99, present.max
        );
        eprintln!(
            "interval {:>6.2}  {:>6.2}  {:>6.2}  {:>6.2}  {:>6.2}   (frame pacing; high p95/p99 = choppy)",
            interval.mean, interval.p50, interval.p95, interval.p99, interval.max
        );
        eprintln!("───────────────────────────");
    }

    fn emit_step_summary(&self) {
        let Report { mode, frames, discarded, fps, render, present, interval, dropped } = self;
        let Ok(path) = std::env::var("GITHUB_STEP_SUMMARY") else {
            return;
        };
        use std::io::Write;
        let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else {
            eprintln!("Skia Bench: failed to open GITHUB_STEP_SUMMARY at {path}");
            return;
        };
        let mut md = String::new();
        md.push_str(&format!("## Skia Bench ({mode})\n\n"));
        md.push_str(&format!(
            "**Achieved FPS:** {fps:.1} &nbsp;·&nbsp; **Frames:** {frames} (discarded {discarded} warm-up) &nbsp;·&nbsp; **Dropped:** {}\n\n",
            dropped_str(*dropped)
        ));
        md.push_str("| Stage | mean | p50 | p95 | p99 | max |\n");
        md.push_str("|-------|------|-----|-----|-----|-----|\n");
        md.push_str(&format!(
            "| render (ms) | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} |\n",
            render.mean, render.p50, render.p95, render.p99, render.max
        ));
        md.push_str(&format!(
            "| present (ms) | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} |\n",
            present.mean, present.p50, present.p95, present.p99, present.max
        ));
        md.push_str(&format!(
            "| interval (ms) | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} |\n\n",
            interval.mean, interval.p50, interval.p95, interval.p99, interval.max
        ));
        md.push_str("_interval = gap between presented frames; high p95/p99 means choppy pacing._\n\n");
        if let Err(e) = f.write_all(md.as_bytes()) {
            eprintln!("Skia Bench: failed to write GITHUB_STEP_SUMMARY: {e}");
        }
    }

    fn emit_notice(&self) {
        let Report { mode, fps, render, present, interval, dropped, .. } = self;
        println!(
            "::notice title=Skia Bench ({mode})::FPS {fps:.1} | render p50 {:.3}ms p95 {:.3}ms | present p50 {:.3}ms p95 {:.3}ms | interval p95 {:.2}ms | dropped {}",
            render.p50, render.p95, present.p50, present.p95, interval.p95, dropped_str(*dropped)
        );
    }
}
