//! Dialog clock and frame-pacing constants.
//!
//! Every time-dependent value a theme widget paints (`anim::animate` tweens, progress animations,
//! the indeterminate phase) is a function of the dialog clock, which core feeds to egui as
//! `RawInput::time`, never of frame counts, so dropped frames never change the motion and the
//! offscreen harness can inject time.

use std::time::{Duration, Instant};

/// Animation cadence (about 60 Hz). Deadlines are anchored to the previous one (skia behaviour).
pub(crate) const FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// Minimum gap between two input-driven / egui-immediate repaints of the same dialog. Every frame
/// is a full egui pass plus a whole-window software rasterization, so input (a continuously
/// moving pointer, key repeat) is coalesced to the animation cadence instead of repainting at up
/// to 250 Hz.
pub(crate) const MIN_GAP: Duration = FRAME_INTERVAL;

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

    /// `Some(t)`: fix the clock at `t`. `None`: resume real time, continuing from the current value
    /// (the clock never jumps backwards when unfreezing).
    pub(crate) fn freeze(&mut self, t: Option<f64>) {
        match t {
            Some(t) => self.frozen = Some(t),
            None => {
                if let Some(cur) = self.frozen.take() {
                    let cur = Duration::from_secs_f64(cur.max(0.0));
                    self.origin = Instant::now().checked_sub(cur).unwrap_or_else(Instant::now);
                }
            }
        }
    }

    pub(crate) fn is_frozen(&self) -> bool {
        self.frozen.is_some()
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
    /// Anchor of the animation cadence (the previous animation deadline).
    anim_anchor: Option<Instant>,
    /// When the last frame was painted.
    last_paint: Option<Instant>,
}

/// What a frame asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Wants {
    /// Input-driven repaint (paint as soon as possible, `MIN_GAP` after the last paint).
    pub immediate: bool,
    /// egui asked for a repaint (`repaint_delay == 0`: a running widget tween / indeterminate bar
    /// called `ctx.request_repaint()`, plus egui's one extra frame after it) or a key-activation
    /// flash is running: next frame at the anchored animation cadence.
    pub animation: bool,
    /// egui's finite `repaint_delay`.
    pub delayed: Option<Duration>,
    /// An explicit clock deadline (e.g. the end of a key-activation flash), relative to now.
    pub at: Option<Duration>,
}

impl Schedule {
    /// Update after a frame painted at `now`.
    pub(crate) fn after_frame(&mut self, now: Instant, wants: Wants) {
        self.last_paint = Some(now);
        let mut next: Option<Instant> = None;
        let mut fold = |t: Instant| next = Some(next.map_or(t, |n: Instant| n.min(t)));
        if wants.animation {
            // Anchor to the previous deadline; resync when more than a frame behind.
            let candidate = self.anim_anchor.map(|a| a + FRAME_INTERVAL);
            let t = match candidate {
                Some(t) if t > now => t,
                _ => now + FRAME_INTERVAL,
            };
            self.anim_anchor = Some(t);
            fold(t);
        } else {
            self.anim_anchor = None;
        }
        if wants.immediate {
            fold(now + MIN_GAP);
        }
        if let Some(d) = wants.delayed {
            if let Some(t) = now.checked_add(d) {
                fold(t);
            }
        }
        if let Some(d) = wants.at {
            if let Some(t) = now.checked_add(d) {
                fold(t);
            }
        }
        self.next = next;
    }

    /// Input arrived: repaint as soon as possible, but not sooner than `MIN_GAP` after the last
    /// paint.
    pub(crate) fn asap(&mut self, now: Instant) {
        let t = self.last_paint.map_or(now, |l| (l + MIN_GAP).max(now));
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
    fn frozen_clock_and_unfreeze_continues() {
        let mut c = DialogClock::frozen(2.5);
        assert_eq!(c.now(), 2.5);
        c.freeze(None);
        let n = c.now();
        assert!((2.5..2.6).contains(&n), "{n}");
        assert!(!c.is_frozen());
    }

    #[test]
    fn animation_cadence_is_anchored() {
        let mut s = Schedule::default();
        let t0 = Instant::now();
        s.after_frame(t0, Wants { animation: true, ..Default::default() });
        assert_eq!(s.next, Some(t0 + FRAME_INTERVAL));
        // A frame painted a bit late keeps the anchor (next = anchor + 16 ms).
        s.after_frame(t0 + Duration::from_millis(18), Wants { animation: true, ..Default::default() });
        assert_eq!(s.next, Some(t0 + FRAME_INTERVAL * 2));
        // Far behind: resync to now + 16 ms.
        let late = t0 + Duration::from_millis(200);
        s.after_frame(late, Wants { animation: true, ..Default::default() });
        assert_eq!(s.next, Some(late + FRAME_INTERVAL));
        // Idle.
        s.after_frame(late, Wants::default());
        assert_eq!(s.next, None);
    }

    #[test]
    fn immediate_and_delayed() {
        let mut s = Schedule::default();
        let t0 = Instant::now();
        s.after_frame(t0, Wants { immediate: true, delayed: Some(Duration::from_secs(1)), ..Default::default() });
        assert_eq!(s.next, Some(t0 + MIN_GAP));
        assert!(s.due(t0 + MIN_GAP));
        s.fired();
        assert!(!s.due(t0 + Duration::from_secs(5)));
    }
}
