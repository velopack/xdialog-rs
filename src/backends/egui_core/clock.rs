//! Dialog clock and frame-pacing constants.
//!
//! Every time-dependent value a theme widget paints (`anim::animate` tweens, progress animations,
//! the indeterminate phase) is a function of the dialog clock, which core feeds to egui as
//! `RawInput::time`, never of frame counts, so dropped frames never change the motion and the
//! offscreen harness can inject time.

use std::time::{Duration, Instant};

/// Frame cadence for ordinary animation and input (60 Hz, or the monitor's rate if that is lower):
/// the soonest a dialog repaints after its previous frame. Every frame is a full egui pass plus a
/// whole-window software rasterization, so fades, value animations and input (a continuously
/// moving pointer, key repeat) are coalesced to it. Continuous motion that asked for smooth frames
/// (`anim::request_smooth_frame`) runs at the monitor's refresh rate instead.
pub(crate) const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

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
    #[cfg(any(test, feature = "_test-hooks"))]
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
    #[cfg(any(test, feature = "_test-hooks"))]
    pub(crate) fn freeze(&mut self, t: f64) {
        self.frozen = Some(t);
    }
}

/// Per-dialog repaint scheduling state.
#[derive(Clone, Debug, Default)]
pub(crate) struct Schedule {
    /// When the dialog wants its next frame (`None` = idle).
    pub next: Option<Instant>,
    /// When the last frame was painted.
    last_paint: Option<Instant>,
    /// The deadline that triggered the frame being painted: the next cadence frame is scheduled
    /// from it, not from the paint time, so wake-up latency and render time don't add up.
    anchor: Option<Instant>,
    /// Refresh period of the dialog's monitor (`None`: unknown, 60 Hz assumed).
    monitor_period: Option<Duration>,
    /// The last frame asked for smooth (refresh-rate) frames.
    smooth: bool,
}

/// What a frame asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Wants {
    /// Repaint at the frame cadence (one more frame after an `egui::Event::PointerGone`).
    pub immediate: bool,
    /// egui's `repaint_delay` (`None` = idle). Zero (a running tween called
    /// `ctx.request_repaint()`) is clamped up to the frame interval.
    pub delay: Option<Duration>,
    /// Continuous motion is on screen: pace at the monitor's refresh rate, not 60 Hz.
    pub smooth: bool,
}

impl Schedule {
    pub(crate) fn set_monitor_period(&mut self, period: Option<Duration>) {
        self.monitor_period = period.filter(|p| *p >= Duration::from_millis(1));
    }

    /// Current frame interval: the monitor's refresh period for smooth frames, otherwise 60 Hz
    /// (never faster than the monitor).
    fn interval(&self) -> Duration {
        match (self.smooth, self.monitor_period) {
            (true, Some(p)) => p,
            (false, Some(p)) => p.max(FRAME_INTERVAL),
            (_, None) => FRAME_INTERVAL,
        }
    }

    /// Update after a frame painted at `now`.
    pub(crate) fn after_frame(&mut self, now: Instant, wants: Wants) {
        self.last_paint = Some(now);
        self.smooth = wants.smooth;
        let interval = self.interval();
        // Keep a steady cadence from the deadline that fired; start over after a missed frame.
        let base = self.anchor.take().filter(|&a| a <= now && now - a < interval).unwrap_or(now);
        let cadence = base + interval;
        let immediate = wants.immediate.then_some(cadence);
        let delayed = wants.delay.map(|d| if d <= interval { cadence } else { now + d });
        self.next = immediate.into_iter().chain(delayed).min();
    }

    /// Input arrived: repaint as soon as possible, but not sooner than one interval after the
    /// last paint.
    pub(crate) fn asap(&mut self, now: Instant) {
        let t = self.last_paint.map_or(now, |l| (l + self.interval()).max(now));
        self.next = Some(self.next.map_or(t, |n| n.min(t)));
    }

    /// A redraw was requested from the window system for this deadline.
    pub(crate) fn fired(&mut self) {
        self.anchor = self.next.take();
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
        s.after_frame(t0, Wants { immediate: true, delay: Some(Duration::from_secs(1)), smooth: false });
        assert_eq!(s.next, Some(t0 + FRAME_INTERVAL));
        assert!(s.due(t0 + FRAME_INTERVAL));
        s.fired();
        assert!(!s.due(t0 + Duration::from_secs(5)));
        // A zero delay (running animation) repaints at the frame cadence.
        s.after_frame(t0, Wants { immediate: false, delay: Some(Duration::ZERO), smooth: false });
        assert_eq!(s.next, Some(t0 + FRAME_INTERVAL));
        s.after_frame(t0, Wants { immediate: false, delay: Some(Duration::from_millis(500)), smooth: false });
        assert_eq!(s.next, Some(t0 + Duration::from_millis(500)));
        // Idle.
        s.after_frame(t0, Wants::default());
        assert_eq!(s.next, None);
        // Input right after a paint waits for the cadence.
        s.asap(t0);
        assert_eq!(s.next, Some(t0 + FRAME_INTERVAL));
    }

    #[test]
    fn cadence_is_anchored_to_the_deadline() {
        let mut s = Schedule::default();
        let t0 = Instant::now();
        let running = Wants { immediate: false, delay: Some(Duration::ZERO), smooth: false };
        s.after_frame(t0, running);
        // Woken and painted 3 ms late: the next frame is still one interval after the deadline.
        let d1 = s.next.unwrap();
        s.fired();
        s.after_frame(d1 + Duration::from_millis(3), running);
        assert_eq!(s.next, Some(d1 + FRAME_INTERVAL));
        // A missed frame restarts the cadence from the paint time.
        let d2 = s.next.unwrap();
        s.fired();
        let late = d2 + Duration::from_millis(40);
        s.after_frame(late, running);
        assert_eq!(s.next, Some(late + FRAME_INTERVAL));
        // A 50 Hz monitor slows the cadence; faster ones keep 60 Hz.
        s.set_monitor_period(Some(Duration::from_millis(20)));
        s.after_frame(late, running);
        assert_eq!(s.next, Some(late + Duration::from_millis(20)));
        s.set_monitor_period(Some(Duration::from_millis(10)));
        assert_eq!(s.interval(), FRAME_INTERVAL);
        // Smooth motion runs at the monitor's rate.
        s.after_frame(late, Wants { smooth: true, ..running });
        assert_eq!(s.next, Some(late + Duration::from_millis(10)));
    }
}
