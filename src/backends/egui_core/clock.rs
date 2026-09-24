//! Dialog clock and frame-pacing constants.
//!
//! Every time-dependent value a theme widget paints (`anim::animate` tweens, progress animations,
//! the indeterminate phase) is a function of the dialog clock, which core feeds to egui as
//! `RawInput::time`, never of frame counts, so dropped frames never change the motion and the
//! offscreen harness can inject time.

use std::time::{Duration, Instant};

/// Frame cadence (about 60 Hz): the soonest a dialog repaints after its previous frame. Every
/// frame is a full egui pass plus a whole-window software rasterization, so animation and input
/// (a continuously moving pointer, key repeat) are coalesced to this cadence.
pub(crate) const FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// Seconds since the dialog was created (real time), or a fixed/injected value (tests).
#[derive(Clone, Debug)]
pub(crate) struct DialogClock {
    origin: Instant,
    frozen: Option<f64>,
}

impl DialogClock {
    /// A real-time clock starting at 0 now.
    pub(crate) fn new() -> Self {
        DialogClock { origin: Instant::now(), frozen: None }
    }

    /// A clock fixed at `t` seconds (offscreen harness).
    pub(crate) fn frozen(t: f64) -> Self {
        DialogClock { origin: Instant::now(), frozen: Some(t) }
    }

    /// Current dialog time in seconds.
    pub(crate) fn now(&self) -> f64 {
        match self.frozen {
            Some(t) => t,
            None => self.origin.elapsed().as_secs_f64(),
        }
    }

    /// Fix the clock at `t` seconds.
    pub(crate) fn freeze(&mut self, t: f64) {
        self.frozen = Some(t);
    }
}

impl Default for DialogClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-dialog repaint scheduling state.
#[derive(Clone, Debug, Default)]
pub(crate) struct Schedule {
    /// When the dialog wants its next frame (`None` = idle).
    pub next: Option<Instant>,
    /// When the last frame was painted.
    last_paint: Option<Instant>,
}

/// What a frame asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Wants {
    /// Repaint at the frame cadence (one more frame after an `egui::Event::PointerGone`).
    pub immediate: bool,
    /// egui's `repaint_delay` (`None` = idle). Zero (a running tween called
    /// `ctx.request_repaint()`) is clamped up to [`FRAME_INTERVAL`].
    pub delay: Option<Duration>,
}

impl Schedule {
    /// Update after a frame painted at `now`.
    pub(crate) fn after_frame(&mut self, now: Instant, wants: Wants) {
        self.last_paint = Some(now);
        let immediate = wants.immediate.then(|| now + FRAME_INTERVAL);
        let delayed = wants.delay.and_then(|d| now.checked_add(d.max(FRAME_INTERVAL)));
        self.next = immediate.into_iter().chain(delayed).min();
    }

    /// Input arrived: repaint as soon as possible, but not sooner than `FRAME_INTERVAL` after the
    /// last paint.
    pub(crate) fn asap(&mut self, now: Instant) {
        let t = self.last_paint.map_or(now, |l| (l + FRAME_INTERVAL).max(now));
        self.next = Some(self.next.map_or(t, |n| n.min(t)));
    }

    /// A redraw was requested from the window system for this deadline.
    pub(crate) fn fired(&mut self) {
        self.next = None;
    }

    pub(crate) fn due(&self, now: Instant) -> bool {
        self.next.is_some_and(|t| t <= now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_clock() {
        let mut c = DialogClock::frozen(2.5);
        assert_eq!(c.now(), 2.5);
        c.freeze(3.0);
        assert_eq!(c.now(), 3.0);
    }

    #[test]
    fn immediate_and_delayed() {
        let mut s = Schedule::default();
        let t0 = Instant::now();
        s.after_frame(t0, Wants { immediate: true, delay: Some(Duration::from_secs(1)) });
        assert_eq!(s.next, Some(t0 + FRAME_INTERVAL));
        assert!(s.due(t0 + FRAME_INTERVAL));
        s.fired();
        assert!(!s.due(t0 + Duration::from_secs(5)));
        // A zero delay (running animation) repaints at the frame cadence.
        s.after_frame(t0, Wants { immediate: false, delay: Some(Duration::ZERO) });
        assert_eq!(s.next, Some(t0 + FRAME_INTERVAL));
        s.after_frame(t0, Wants { immediate: false, delay: Some(Duration::from_millis(500)) });
        assert_eq!(s.next, Some(t0 + Duration::from_millis(500)));
        // Idle.
        s.after_frame(t0, Wants::default());
        assert_eq!(s.next, None);
        // Input right after a paint waits for the cadence.
        s.asap(t0);
        assert_eq!(s.next, Some(t0 + FRAME_INTERVAL));
    }
}
