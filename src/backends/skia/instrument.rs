//! Per-frame render/present instrumentation for the skia backend.
//!
//! Entirely feature-gated behind `skia-instrumentation`; compiles out of the shipping crate.
//!
//! The skia event loop, `render_and_present`, and `report()` all run on the same thread (the one
//! that called `run_loop`), so the collector is a `thread_local!` `RefCell<Recorder>` — no locks,
//! and no `&mut` threaded through every call site. Driven by `examples/skia_bench.rs`.
//!
//! Two independent streams are recorded:
//! * **paints** — one [`FrameSample`] per actual `render_and_present`, carrying the paint and
//!   present durations. Used for the render/present timing stats (and, in uncapped mode, for the
//!   inter-frame interval and FPS, since uncapped has no fixed schedule).
//! * **ticks** — one [`TickSample`] per *scheduled* animation frame in capped mode, tagged with
//!   whether that frame actually painted. The capped loop wakes on a fixed ~60fps cadence and ticks
//!   every frame, but a frame only paints when something visibly changed. The indeterminate spinner
//!   deliberately parks the capsule at each end for ~0.22s per half-cycle; during those windows the
//!   cadence keeps ticking but nothing repaints. Measuring pacing from *paints* would read those
//!   intentional idle windows as ~220ms stalls / dropped frames (and they only show up at all on
//!   headless Linux, where — unlike macOS — the compositor issues no spontaneous expose redraws to
//!   paper over them). Measuring from *ticks* reports the true cadence and surfaces the idle
//!   separately as a "painted" percentage.

use std::cell::RefCell;
use std::time::{Duration, Instant};

/// One actual painted (or expose-re-presented) frame: how long the component paint loop took, how
/// long the present took, and when it was recorded.
struct FrameSample {
    render: Duration,
    present: Duration,
    at: Instant,
}

/// One scheduled animation frame on the capped 60fps cadence. `painted` is whether this frame
/// produced a redraw (something changed) or was a parked/idle tick (e.g. a spinner end-pause).
struct TickSample {
    at: Instant,
    painted: bool,
}

struct Recorder {
    paints: Vec<FrameSample>,
    ticks: Vec<TickSample>,
    /// Whether the loop is running uncapped (max-throughput). Resolved once from the environment.
    uncapped: bool,
}

thread_local! {
    static RECORDER: RefCell<Recorder> = RefCell::new(Recorder {
        paints: Vec::new(),
        ticks: Vec::new(),
        uncapped: std::env::var("XDIALOG_BENCH_UNCAPPED").map(|v| v == "1").unwrap_or(false),
    });
}

/// Number of initial samples discarded before computing stats: first pixmap alloc, initial layout,
/// font shaping, and surface warm-up all land here and would skew the percentiles. Applied
/// independently to each stream.
const WARMUP_FRAMES: usize = 30;

/// The capped loop's frame period (matches `FRAME_TIME` in `about_to_wait`). Used to translate
/// tick-interval overruns into a dropped-frame count.
const FRAME_MS: f64 = 16.0;

/// Whether the event loop should run uncapped (`ControlFlow::Poll`) instead of the 60fps cap.
/// Reads `XDIALOG_BENCH_UNCAPPED` (cached on first access).
pub fn uncapped() -> bool {
    RECORDER.with(|r| r.borrow().uncapped)
}

/// Record one painted frame. `present`-only (expose) frames pass a zero render duration.
pub fn record_frame(render: Duration, present: Duration) {
    let at = Instant::now();
    RECORDER.with(|r| {
        r.borrow_mut().paints.push(FrameSample { render, present, at });
    });
}

/// Record one scheduled animation frame on the capped cadence. `painted` is whether the frame
/// produced a redraw this tick (vs. a parked/idle frame such as a spinner end-pause). Not called in
/// uncapped mode, which has no fixed schedule.
pub fn record_tick(painted: bool) {
    let at = Instant::now();
    RECORDER.with(|r| {
        r.borrow_mut().ticks.push(TickSample { at, painted });
    });
}

/// Compute statistics over the recorded frames and emit them to stderr, the GitHub step summary
/// (if `GITHUB_STEP_SUMMARY` is set), and as a `::notice` annotation.
pub fn report() {
    RECORDER.with(|r| {
        let rec = r.borrow();
        let report = if rec.uncapped {
            build_uncapped(&rec)
        } else {
            build_capped(&rec)
        };
        match report {
            Ok(report) => {
                report.emit_stderr();
                report.emit_step_summary();
                report.emit_notice();
            }
            Err(msg) => eprintln!("{msg}"),
        }
    });
}

/// Uncapped (max-throughput): there is no fixed schedule, so pacing/FPS are derived from the
/// painted frames themselves. `painted`/`dropped` don't apply.
fn build_uncapped(rec: &Recorder) -> Result<Report, String> {
    let total = rec.paints.len();
    if total <= WARMUP_FRAMES {
        return Err(format!(
            "Skia Bench (uncapped): only {total} frames recorded (<= {WARMUP_FRAMES} warm-up); \
             nothing to report."
        ));
    }
    let paints = &rec.paints[WARMUP_FRAMES..];
    let frames = paints.len();

    let render = Stats::compute(paints.iter().map(|s| s.render));
    let present = Stats::compute(paints.iter().map(|s| s.present));
    let interval = Stats::compute(paints.windows(2).map(|w| w[1].at.duration_since(w[0].at)));

    let elapsed = paints[frames - 1].at.duration_since(paints[0].at).as_secs_f64();
    let fps = if elapsed > 0.0 { frames as f64 / elapsed } else { 0.0 };

    Ok(Report {
        mode: "uncapped",
        frames,
        discarded: WARMUP_FRAMES,
        fps,
        render,
        present,
        interval,
        painted: None,
        dropped: None,
    })
}

/// Capped (~60fps): pacing/FPS/dropped are derived from the *scheduled ticks* so intentional idle
/// (spinner end-pauses) is reported as a lower `painted` percentage rather than mis-read as stalls.
/// Render/present timings still come from the painted frames.
fn build_capped(rec: &Recorder) -> Result<Report, String> {
    let total = rec.ticks.len();
    if total <= WARMUP_FRAMES {
        return Err(format!(
            "Skia Bench (capped): only {total} ticks recorded (<= {WARMUP_FRAMES} warm-up); \
             nothing to report."
        ));
    }
    let ticks = &rec.ticks[WARMUP_FRAMES..];
    let frames = ticks.len();

    // Pacing: gap between consecutive *scheduled* frames. The anchored cadence keeps this near the
    // frame period regardless of whether each frame painted, so a high p95/p99 here is a genuine
    // late wake-up, not idle.
    let gaps_ms: Vec<f64> = ticks
        .windows(2)
        .map(|w| w[1].at.duration_since(w[0].at).as_secs_f64() * 1000.0)
        .collect();
    let interval = Stats::from_ms(gaps_ms.clone());

    // Dropped frames = scheduled slots skipped because the loop woke late. A clean 16ms gap counts
    // 0; a 33ms hitch counts 1, etc. Idle/parked frames still tick on time, so they count 0.
    let dropped: i64 = gaps_ms
        .iter()
        .map(|&ms| (ms / FRAME_MS).round() as i64 - 1)
        .filter(|&d| d > 0)
        .sum();

    let painted_count = ticks.iter().filter(|t| t.painted).count();
    let painted_pct = 100.0 * painted_count as f64 / frames as f64;

    let elapsed = ticks[frames - 1].at.duration_since(ticks[0].at).as_secs_f64();
    let fps = if elapsed > 0.0 { frames as f64 / elapsed } else { 0.0 };

    // Render/present timings come from the painted frames (zero-duration expose re-presents and the
    // warm-up paints are dropped so the percentiles reflect real paint cost).
    let paints: &[FrameSample] = if rec.paints.len() > WARMUP_FRAMES {
        &rec.paints[WARMUP_FRAMES..]
    } else {
        &rec.paints
    };
    let painted_only = || paints.iter().filter(|s| s.render > Duration::ZERO);
    let render = Stats::compute(painted_only().map(|s| s.render));
    let present = Stats::compute(painted_only().map(|s| s.present));

    Ok(Report {
        mode: "capped",
        frames,
        discarded: WARMUP_FRAMES,
        fps,
        render,
        present,
        interval,
        painted: Some(painted_pct),
        dropped: Some(dropped),
    })
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
    /// Capped mode only: percentage of scheduled frames that actually painted (vs. parked idle).
    painted: Option<f64>,
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
        Self::from_ms(durs.map(|d| d.as_secs_f64() * 1000.0).collect())
    }

    fn from_ms(mut ms: Vec<f64>) -> Self {
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

fn painted_str(painted: Option<f64>) -> String {
    painted.map(|p| format!("{p:.1}%")).unwrap_or_else(|| "N/A".to_string())
}

impl Report {
    fn emit_stderr(&self) {
        let Report { mode, frames, discarded, fps, render, present, interval, painted, dropped } = self;
        eprintln!();
        eprintln!("─── Skia Bench ({mode}) ───");
        eprintln!(
            "frames: {frames} (discarded {discarded} warm-up)   achieved FPS: {fps:.1}   painted: {}   dropped: {}",
            painted_str(*painted),
            dropped_str(*dropped)
        );
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
        let Report { mode, frames, discarded, fps, render, present, interval, painted, dropped } = self;
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
            "**Achieved FPS:** {fps:.1} &nbsp;·&nbsp; **Frames:** {frames} (discarded {discarded} warm-up) \
             &nbsp;·&nbsp; **Painted:** {} &nbsp;·&nbsp; **Dropped:** {}\n\n",
            painted_str(*painted),
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
        md.push_str(
            "_interval = gap between scheduled frames (frame pacing); high p95/p99 means choppy. \
             painted = share of scheduled frames that actually repainted; the rest are intentional \
             idle (e.g. spinner end-pauses), not dropped frames._\n\n",
        );
        if let Err(e) = f.write_all(md.as_bytes()) {
            eprintln!("Skia Bench: failed to write GITHUB_STEP_SUMMARY: {e}");
        }
    }

    fn emit_notice(&self) {
        let Report { mode, fps, render, present, interval, painted, dropped, .. } = self;
        println!(
            "::notice title=Skia Bench ({mode})::FPS {fps:.1} | render p50 {:.3}ms p95 {:.3}ms | \
             present p50 {:.3}ms p95 {:.3}ms | interval p95 {:.2}ms | painted {} | dropped {}",
            render.p50, render.p95, present.p50, present.p95, interval.p95,
            painted_str(*painted),
            dropped_str(*dropped)
        );
    }
}
