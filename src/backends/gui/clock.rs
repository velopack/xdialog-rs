//! Dialog clock and frame-pacing constants.
//!
//! Every time-dependent value a theme widget paints (`Ui::animate` tweens, progress animations,
//! the indeterminate phase) is a function of the dialog clock (`Ui::time`), never of frame
//! counts, so dropped frames never change the motion and the offscreen harness can inject time.

use std::time::{Duration, Instant};

/// Frame cadence: 60 Hz, or the monitor's rate if lower. Each frame is a full theme pass plus a
/// whole-window software rasterization, so animation and input (pointer motion, key repeat) are
/// coalesced to it. Smooth motion (`Ui::request_smooth_frame`) runs at the monitor's rate instead.
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
    /// Another frame at the cadence (a tween is running, the pointer just left, ...); `false` =
    /// idle.
    pub repaint: bool,
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
        self.next = wants.repaint.then_some(base + interval);
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

    /// The requested frame was not painted (window hidden): the next one counts as the window
    /// system's.
    pub(crate) fn skipped(&mut self) {
        self.anchor = None;
    }

    /// The frame being painted is one this schedule asked for ([`Schedule::fired`]), not one the
    /// window system needs (expose, resize, first show).
    pub(crate) fn self_scheduled(&self) -> bool {
        self.anchor.is_some()
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
    fn repaint_and_idle() {
        let mut s = Schedule::default();
        let t0 = Instant::now();
        s.after_frame(t0, Wants { repaint: true, smooth: false });
        assert_eq!(s.next, Some(t0 + FRAME_INTERVAL));
        assert!(s.due(t0 + FRAME_INTERVAL));
        assert!(!s.self_scheduled());
        s.fired();
        assert!(s.self_scheduled());
        assert!(!s.due(t0 + Duration::from_secs(5)));
        s.after_frame(t0, Wants::default());
        assert_eq!(s.next, None);
        assert!(!s.self_scheduled());
        // Input right after a paint waits for the cadence.
        s.asap(t0);
        assert_eq!(s.next, Some(t0 + FRAME_INTERVAL));
    }

    #[test]
    fn cadence_is_anchored_to_the_deadline() {
        let mut s = Schedule::default();
        let t0 = Instant::now();
        let running = Wants { repaint: true, smooth: false };
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
