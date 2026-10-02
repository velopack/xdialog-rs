//! Easing curves, transitions and the per-dialog tween store behind [`Ui::animate`].
//!
//! Every tween is a pure function of the dialog clock (see `clock.rs`), never of frame counts.
//!
//! [`Ui::animate`]: super::ui::Ui::animate

use std::any::Any;
use std::collections::HashMap;

use super::ui::Id;
use crate::backends::draw::Color;

/// An easing curve mapping linear progress `t` in 0..=1 to eased progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Easing {
    Linear,
    /// CSS `cubic-bezier(x1, y1, x2, y2)` / XAML KeySpline semantics.
    CubicBezier(f32, f32, f32, f32),
    /// `1 - (1 - t)^3`.
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
        if t <= 0.0 || t >= 1.0 {
            return t;
        }
        match self {
            Easing::Linear => t,
            Easing::OutCubic => 1.0 - (1.0 - t).powi(3),
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

/// Values a tween can interpolate. Implement it for a theme's own "look" structs (e.g. a button's
/// border/fill/text colours) to fade all of them with one tween.
pub(crate) trait Lerp: Clone + PartialEq + 'static {
    /// `a` at `t == 0`, `b` at `t == 1`.
    fn lerp(a: &Self, b: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(a: &f32, b: &f32, t: f32) -> f32 {
        a + (b - a) * t
    }
}

impl Lerp for Color {
    fn lerp(a: &Color, b: &Color, t: f32) -> Color {
        a.lerp_to_gamma(*b, t)
    }
}

/// One running (or finished) transition.
#[derive(Clone, Debug)]
struct Tween<T> {
    from: T,
    to: T,
    start: f64,
    tr: Transition,
}

impl<T: Lerp> Tween<T> {
    fn snapped(value: T, now: f64) -> Self {
        Tween { from: value.clone(), to: value, start: now, tr: Transition::INSTANT }
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

/// Travel position 0..1 of the indeterminate "stretchy capsule" (see
/// [`indeterminate_capsule`](super::ui::indeterminate_capsule)) at normalized cycle time `n`:
/// 0-40 % sweep right, 40-50 % hold, 50-90 % sweep back, 90-100 % hold; each sweep eases with
/// smoothstep.
pub(crate) fn capsule_pos(n: f32) -> f32 {
    let smooth = |t: f32| {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    if n < 0.4 {
        smooth(n / 0.4)
    } else if n < 0.5 {
        1.0
    } else if n < 0.9 {
        1.0 - smooth((n - 0.5) / 0.4)
    } else {
        0.0
    }
}

/// The tweens of one dialog, keyed by widget [`Id`].
#[derive(Default)]
pub(crate) struct Tweens {
    map: HashMap<Id, Box<dyn Any>>,
}

impl Tweens {
    /// Interruptible tween of `id` at time `now`; returns the value and whether it still runs.
    ///
    /// - The first call for `id` snaps to `target` (no animation on open).
    /// - When `target` changes, the value animates from the *currently displayed* value to the
    ///   new target over `tr` (an interrupted fade reverses smoothly); on the call of the change
    ///   the old displayed value is returned (elapsed = 0).
    pub(crate) fn animate<T: Lerp>(&mut self, id: Id, now: f64, target: T, tr: Transition) -> (T, bool) {
        let slot = self.map.entry(id).or_insert_with(|| Box::new(Tween::snapped(target.clone(), now)));
        let tw = match slot.downcast_mut::<Tween<T>>() {
            Some(tw) => tw,
            None => {
                *slot = Box::new(Tween::snapped(target.clone(), now));
                slot.downcast_mut::<Tween<T>>().expect("just inserted")
            }
        };
        if tw.to != target {
            *tw = Tween { from: tw.value(now), to: target, start: now, tr };
        }
        (tw.value(now), tw.active(now))
    }

    /// Stop the tween of `id` at the value it displays now (a later `animate` with a new target
    /// starts from there).
    pub(crate) fn stop<T: Lerp>(&mut self, id: Id, now: f64) {
        if let Some(tw) = self.map.get_mut(&id).and_then(|t| t.downcast_mut::<Tween<T>>()) {
            *tw = Tween::snapped(tw.value(now), now);
        }
    }

    /// Forget every tween: the next `animate` of each id snaps (appearance change, open).
    pub(crate) fn reset(&mut self) {
        self.map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_matches_css_reference() {
        // easeOutQuad as cubic-bezier(0.25, 0.46, 0.45, 0.94) at x = 0.5 -> y ~= 0.7713
        let e = Easing::CubicBezier(0.25, 0.46, 0.45, 0.94);
        assert!((e.apply(0.5) - 0.7713).abs() < 1e-3, "{}", e.apply(0.5));
        let e = Easing::FLUENT_FAST_OUT_SLOW_IN;
        let mut prev = 0.0;
        for i in 0..=100 {
            let v = e.apply(i as f32 / 100.0);
            assert!(v + 1e-6 >= prev);
            prev = v;
        }
        assert_eq!((e.apply(0.0), e.apply(1.0)), (0.0, 1.0));
    }

    #[test]
    fn tween_snaps_then_interrupts_from_displayed() {
        let mut tw = Tweens::default();
        let id = Id::new("t");
        let tr = Transition::linear(0.2);
        assert_eq!(tw.animate(id, 0.0, 0.0f32, tr), (0.0, false));
        assert_eq!(tw.animate(id, 1.0, 1.0f32, tr), (0.0, true));
        assert!((tw.animate(id, 1.1, 1.0f32, tr).0 - 0.5).abs() < 1e-5);
        // Interrupt at 1.15 (displayed 0.75): back to 0 over 0.2 s.
        assert!((tw.animate(id, 1.15, 0.0f32, tr).0 - 0.75).abs() < 1e-5);
        assert!((tw.animate(id, 1.25, 0.0f32, tr).0 - 0.375).abs() < 1e-5);
        assert_eq!(tw.animate(id, 1.36, 0.0f32, tr), (0.0, false));
    }

    #[test]
    fn stop_and_reset() {
        let mut tw = Tweens::default();
        let id = Id::new("p");
        let tr = Transition::linear(0.2);
        tw.animate(id, 0.0, 0.0f32, tr);
        tw.animate(id, 1.0, 1.0f32, tr);
        tw.stop::<f32>(id, 1.1);
        assert!((tw.animate(id, 5.0, 0.5f32, tr).0 - 0.5).abs() < 1e-5, "frozen at the displayed value");
        tw.animate(id, 6.0, 1.0f32, tr);
        assert!((tw.animate(id, 6.1, 1.0f32, tr).0 - 0.75).abs() < 1e-5);
        tw.reset();
        assert_eq!(tw.animate(id, 7.0, Color::BLACK, tr), (Color::BLACK, false));
    }

    #[test]
    fn capsule_timeline() {
        assert_eq!(capsule_pos(0.0), 0.0);
        assert!((capsule_pos(0.2) - 0.5).abs() < 1e-6);
        assert_eq!(capsule_pos(0.45), 1.0);
        assert!((capsule_pos(0.7) - 0.5).abs() < 1e-6);
        assert_eq!(capsule_pos(0.95), 0.0);
    }

    #[test]
    fn reset_snaps_colors() {
        let mut tw = Tweens::default();
        let id = Id::new("c");
        let tr = Transition::linear(0.15);
        tw.animate(id, 0.0, Color::WHITE, tr);
        tw.reset();
        assert_eq!(tw.animate(id, 0.5, Color::BLACK, tr), (Color::BLACK, false));
        assert_eq!(tw.animate(id, 0.6, Color::WHITE, Transition::INSTANT), (Color::WHITE, false));
    }
}
