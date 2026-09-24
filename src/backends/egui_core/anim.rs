//! Easing curves, transitions and widget-local tweens.
//!
//! There is no core tween store: a theme widget animates its own state with
//! [`animate`] (state lives in egui's `IdTypeMap`, keyed by the widget's `Id`) or with egui's own
//! `ctx.animate_value_with_time` / `animate_bool_with_time_and_easing`.
//!
//! [`animate`] is a pure function of the egui input time (`RawInput::time`, which core sets from
//! the dialog clock), never of frame counts, so dropped frames never change the motion and the
//! offscreen harness is deterministic.

use egui::{Color32, Id};

/// An easing curve mapping linear progress `t` in 0..=1 to eased progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Easing {
    Linear,
    /// CSS `cubic-bezier(x1, y1, x2, y2)` / XAML KeySpline semantics.
    CubicBezier(f32, f32, f32, f32),
    /// `1 - (1 - t)^3` (skia progress value animation, mina `OutCubic`).
    OutCubic,
}

impl Easing {
    /// WinUI ControlFastOutSlowInKeySpline.
    pub const FLUENT_FAST_OUT_SLOW_IN: Easing = Easing::CubicBezier(0.0, 0.0, 0.0, 1.0);
    /// WinUI indeterminate ProgressBar key spline.
    pub const FLUENT_PROGRESS: Easing = Easing::CubicBezier(0.4, 0.0, 0.6, 1.0);

    /// Apply the curve. `t` is clamped to 0..=1; the result is exactly 0 at 0 and 1 at 1.
    pub fn apply(self, t: f32) -> f32 {
        let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
        if t <= 0.0 {
            return 0.0;
        }
        if t >= 1.0 {
            return 1.0;
        }
        match self {
            Easing::Linear => t,
            Easing::OutCubic => {
                let u = 1.0 - t;
                1.0 - u * u * u
            }
            Easing::CubicBezier(x1, y1, x2, y2) => cubic_bezier(x1, y1, x2, y2, t),
        }
    }
}

/// Solve the CSS cubic-bezier for x = `x`, return y. Newton iterations with a bisection fallback.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    // B(s) = 3(1-s)^2 s P1 + 3(1-s) s^2 P2 + s^3, with P0 = 0 and P3 = 1.
    let (x1, y1, x2, y2, x) = (x1 as f64, y1 as f64, x2 as f64, y2 as f64, x as f64);
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;
    let sample_x = |s: f64| ((ax * s + bx) * s + cx) * s;
    let sample_y = |s: f64| ((ay * s + by) * s + cy) * s;
    let slope_x = |s: f64| (3.0 * ax * s + 2.0 * bx) * s + cx;

    let mut s = x;
    let mut solved = false;
    for _ in 0..8 {
        let err = sample_x(s) - x;
        if err.abs() < 1e-7 {
            solved = true;
            break;
        }
        let d = slope_x(s);
        if d.abs() < 1e-7 {
            break;
        }
        s -= err / d;
    }
    if !solved || !(0.0..=1.0).contains(&s) {
        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        s = x;
        for _ in 0..64 {
            let v = sample_x(s);
            if (v - x).abs() < 1e-7 {
                break;
            }
            if v < x {
                lo = s;
            } else {
                hi = s;
            }
            s = (lo + hi) / 2.0;
        }
    }
    sample_y(s) as f32
}

/// A transition: duration in seconds (0 = instant) plus easing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Transition {
    pub duration: f32,
    pub easing: Easing,
}

impl Transition {
    pub const INSTANT: Transition = Transition { duration: 0.0, easing: Easing::Linear };

    pub const fn new(duration: f32, easing: Easing) -> Self {
        Transition { duration, easing }
    }

    pub const fn linear(duration: f32) -> Self {
        Transition { duration, easing: Easing::Linear }
    }

    /// Eased progress at `elapsed` seconds after the start (1.0 when instant or finished).
    pub fn progress(&self, elapsed: f64) -> f32 {
        if self.duration <= 0.0 {
            return 1.0;
        }
        self.easing.apply((elapsed / self.duration as f64) as f32)
    }
}

// ------------------------------------------------------------------------------------------------
// Widget-local tweens
// ------------------------------------------------------------------------------------------------

/// Values a widget-local tween can interpolate. Implement it for a theme's own "look" structs
/// (e.g. a button's border/fill/text colours) to fade all of them with one tween, as the skia
/// backend's `animator!` did.
pub(crate) trait Lerp: Clone + PartialEq + Send + Sync + 'static {
    /// `a` at `t == 0`, `b` at `t == 1`.
    fn lerp(a: &Self, b: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(a: &f32, b: &f32, t: f32) -> f32 {
        a + (b - a) * t
    }
}

impl Lerp for Color32 {
    /// Per channel in gamma (sRGB) space.
    fn lerp(a: &Color32, b: &Color32, t: f32) -> Color32 {
        a.lerp_to_gamma(*b, t)
    }
}

/// One running (or finished) transition, stored in egui memory under the widget's id.
#[derive(Clone, Debug)]
struct Tween<T> {
    from: T,
    to: T,
    start: f64,
    tr: Transition,
    generation: u64,
}

impl<T: Lerp> Tween<T> {
    fn snapped(value: T, now: f64, generation: u64) -> Self {
        Tween { from: value.clone(), to: value, start: now, tr: Transition::INSTANT, generation }
    }

    fn value(&self, now: f64) -> T {
        if self.active(now) {
            T::lerp(&self.from, &self.to, self.tr.progress(now - self.start))
        } else {
            self.to.clone()
        }
    }

    fn active(&self, now: f64) -> bool {
        self.tr.duration > 0.0 && now - self.start < self.tr.duration as f64
    }
}

fn generation_id() -> Id {
    Id::new("xdialog.anim.generation")
}

fn generation(ctx: &egui::Context) -> u64 {
    ctx.data(|d| d.get_temp::<u64>(generation_id())).unwrap_or(0)
}

/// Widget-local interruptible tween, keyed by `id` (use the widget's `Id`, or `id.with("part")`
/// for several tweens per widget).
///
/// - The first call for `id` snaps to `target` (no animation on open).
/// - When `target` changes, the value animates from the *currently displayed* value to the new
///   target over `tr` (skia `blend_next_timeline` semantics: an interrupted fade reverses
///   smoothly). On the pass of the change the old displayed value is returned (elapsed = 0).
/// - Time is `ctx.input(|i| i.time)` (the injected dialog clock). While the transition runs it
///   calls `ctx.request_repaint()`; core turns that into frames at the animation cadence.
/// - Calling it again in the same pass with the same target is idempotent.
pub(crate) fn animate<T: Lerp>(ctx: &egui::Context, id: Id, target: T, tr: Transition) -> T {
    let now = ctx.input(|i| i.time);
    let gen = generation(ctx);
    let key = id.with("xdialog.anim");
    let (value, active) = ctx.data_mut(|d| {
                                 let tw = d.get_temp_mut_or_insert_with(key, || Tween::snapped(target.clone(), now, gen));
                                 if tw.generation != gen {
                                     *tw = Tween::snapped(target.clone(), now, gen);
                                 } else if tw.to != target {
                                     let current = tw.value(now);
                                     *tw = Tween { from: current, to: target.clone(), start: now, tr, generation: gen };
                                 }
                                 (tw.value(now), tw.active(now))
                             });
    if active {
        ctx.request_repaint();
    }
    value
}

/// Stop the tween of `id` at the value it displays now (a later [`animate`] call with a new target
/// starts from there) and return that value; `None` if `id` has no tween of type `T` yet. Use it
/// to freeze a progress value tween while the bar is indeterminate.
pub(crate) fn stop<T: Lerp>(ctx: &egui::Context, id: Id) -> Option<T> {
    let now = ctx.input(|i| i.time);
    let key = id.with("xdialog.anim");
    ctx.data_mut(|d| {
           let tw = d.get_temp::<Tween<T>>(key)?;
           let value = tw.value(now);
           d.insert_temp(key, Tween::snapped(value.clone(), now, tw.generation));
           Some(value)
       })
}

/// Make the next [`animate`] call of every id snap to its target, and clear egui's own
/// animations. Core calls this on theme/appearance change so colours don't fade between palettes.
pub(crate) fn reset_animations(ctx: &egui::Context) {
    let next = generation(ctx).wrapping_add(1);
    ctx.data_mut(|d| d.insert_temp(generation_id(), next));
    ctx.clear_animations();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_matches_css_reference() {
        // easeOutQuad as cubic-bezier(0.25, 0.46, 0.45, 0.94) at x = 0.5 -> y ~= 0.7713 (bisection reference)
        let e = Easing::CubicBezier(0.25, 0.46, 0.45, 0.94);
        assert!((e.apply(0.5) - 0.7713).abs() < 1e-3, "{}", e.apply(0.5));
        // (0,0,0,1) is monotonic
        let e = Easing::FLUENT_FAST_OUT_SLOW_IN;
        let mut prev = 0.0;
        for i in 0..=100 {
            let v = e.apply(i as f32 / 100.0);
            assert!(v + 1e-6 >= prev);
            prev = v;
        }
        assert_eq!(e.apply(0.0), 0.0);
        assert_eq!(e.apply(1.0), 1.0);
    }

    /// Run one egui pass at time `t`; returns what `f` returned and whether a repaint was asked.
    fn at<R>(ctx: &egui::Context, t: f64, mut f: impl FnMut(&egui::Context) -> R) -> (R, bool) {
        let mut out = None;
        let raw = egui::RawInput { time: Some(t), ..Default::default() };
        let mut full = ctx.run_ui(raw, |ui| out = Some(f(ui.ctx())));
        full.textures_delta.clear();
        let repaint = full.viewport_output.values().any(|v| v.repaint_delay.is_zero());
        (out.unwrap(), repaint)
    }

    #[test]
    fn tween_snaps_then_interrupts_from_displayed() {
        let ctx = egui::Context::default();
        let id = Id::new("t");
        let tr = Transition::linear(0.2);
        assert_eq!(at(&ctx, 0.0, |c| animate(c, id, 0.0f32, tr)).0, 0.0);
        let (v, active) = at(&ctx, 1.0, |c| animate(c, id, 1.0f32, tr));
        assert_eq!(v, 0.0);
        assert!(active);
        let (v, _) = at(&ctx, 1.1, |c| animate(c, id, 1.0f32, tr));
        assert!((v - 0.5).abs() < 1e-5);
        // Interrupt at 1.15 (displayed 0.75): back to 0 over 0.2 s.
        let (v, _) = at(&ctx, 1.15, |c| animate(c, id, 0.0f32, tr));
        assert!((v - 0.75).abs() < 1e-5);
        let (v, _) = at(&ctx, 1.25, |c| animate(c, id, 0.0f32, tr));
        assert!((v - 0.375).abs() < 1e-5);
        let (v, _) = at(&ctx, 1.36, |c| animate(c, id, 0.0f32, tr));
        assert_eq!(v, 0.0);
        // egui repaints once more after the last request_repaint, then goes idle.
        let (v, active) = at(&ctx, 1.4, |c| animate(c, id, 0.0f32, tr));
        assert_eq!((v, active), (0.0, false));
    }

    #[test]
    fn stop_freezes_at_displayed_value() {
        let ctx = egui::Context::default();
        let id = Id::new("p");
        let tr = Transition::linear(0.2);
        assert_eq!(at(&ctx, 0.0, |c| stop::<f32>(c, id)).0, None);
        at(&ctx, 0.0, |c| animate(c, id, 0.0f32, tr));
        at(&ctx, 1.0, |c| animate(c, id, 1.0f32, tr));
        let (v, _) = at(&ctx, 1.1, |c| stop::<f32>(c, id));
        assert!((v.unwrap() - 0.5).abs() < 1e-5);
        // Frozen: still 0.5 later; a new target starts from there.
        assert!((at(&ctx, 5.0, |c| animate(c, id, 0.5f32, tr)).0 - 0.5).abs() < 1e-5);
        at(&ctx, 6.0, |c| animate(c, id, 1.0f32, tr));
        assert!((at(&ctx, 6.1, |c| animate(c, id, 1.0f32, tr)).0 - 0.75).abs() < 1e-5);
    }

    #[test]
    fn reset_snaps_colors() {
        let ctx = egui::Context::default();
        let id = Id::new("c");
        let tr = Transition::linear(0.15);
        at(&ctx, 0.0, |c| animate(c, id, Color32::WHITE, tr));
        reset_animations(&ctx);
        let (v, _) = at(&ctx, 0.5, |c| animate(c, id, Color32::BLACK, tr));
        assert_eq!(v, Color32::BLACK);
        let (v, _) = at(&ctx, 0.6, |c| animate(c, id, Color32::WHITE, Transition::INSTANT));
        assert_eq!(v, Color32::WHITE);
    }
}
