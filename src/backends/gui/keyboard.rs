//! Core keyboard policy: moves focus between the theme's buttons, decides keyboard activation and
//! Escape, and tells widgets what they need through `FrameInfo` (focus-visible modality, the
//! keyboard-pressed button, scroll requests).
//!
//! It works on the previous pass's `DialogUiOutput` (the buttons in Tab = arrow order; the default
//! button is the highest API index), and is driven event by event, before the next pass, by
//! `dialog.rs`. `focus` is the dialog's focused button (API index).

use super::input::{Event, Key, PointerButton};
use super::theme::{ArrowNav, DialogUiOutput, FocusVisibility, FrameInfo, KeyboardPolicy, SpaceKey};

/// What a key asks the dialog to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyAction {
    None,
    /// Activate the button with this API index (same path as a pointer click).
    Activate(usize),
    /// Escape: close the dialog (`WindowClosed`).
    Close,
}

/// Scroll step for Up/Down, logical px.
const LINE_SCROLL: f64 = 40.0;
/// PageUp/PageDown scroll this fraction of the client height.
const PAGE_FRACTION: f64 = 0.9;
/// Home/End scroll: a large finite value (the theme clamps).
const END_SCROLL: f64 = 1.0e6;

/// Per-dialog keyboard state.
#[derive(Clone, Debug)]
pub(crate) struct KeyboardState {
    policy: KeyboardPolicy,
    focus_visible: bool,
    /// Space pressed on this button (`SpaceKey::ActivateOnRelease`), not released yet.
    space_down: Option<usize>,
    /// Accumulated keyboard scroll for the next pass.
    scroll: f64,
    /// Accumulated wheel scroll for the next pass (applied under the pointer only).
    wheel: f64,
}

impl KeyboardState {
    pub(crate) fn new(policy: KeyboardPolicy) -> Self {
        KeyboardState { policy, focus_visible: false, space_down: None, scroll: 0.0, wheel: 0.0 }
    }

    /// The on-open step, after the measure pass: focus the default button, focus visible.
    pub(crate) fn on_open(&mut self, focus: &mut Option<usize>, out: &DialogUiOutput) {
        *focus = default_button(out);
        self.focus_visible = true;
    }

    /// Wheel scrolling (positive `dy`: reveal content further down).
    pub(crate) fn wheel(&mut self, dy: f64) {
        self.wheel += dy;
    }

    /// Process one input event (before the next pass): keys, primary presses (focus-visible
    /// modality) and window focus loss. `client_h` is the client height (page scrolling).
    pub(crate) fn on_event(&mut self, ev: &Event, focus: &mut Option<usize>, out: &DialogUiOutput, client_h: f64) -> KeyAction {
        match *ev {
            Event::WindowFocused(false) => self.space_down = None,
            Event::PointerButton { button: PointerButton::Primary, pressed: true, .. }
                if self.policy.focus_visibility == FocusVisibility::KeyboardOnly =>
            {
                self.focus_visible = false
            }
            Event::Key { key, pressed, repeat, shift } => return self.on_key(key, pressed, repeat, shift, focus, out, client_h),
            _ => {}
        }
        KeyAction::None
    }

    #[allow(clippy::too_many_arguments)]
    fn on_key(&mut self,
              key: Key,
              pressed: bool,
              repeat: bool,
              shift: bool,
              focus: &mut Option<usize>,
              out: &DialogUiOutput,
              client_h: f64)
              -> KeyAction {
        let p = self.policy;
        let focused = focused_button(*focus, out);
        let order: Vec<usize> = out.buttons.iter().map(|b| b.index).collect();
        if !pressed {
            // Only Space-release matters.
            if key == Key::Space && p.space == SpaceKey::ActivateOnRelease {
                if let Some(b) = self.space_down.take().filter(|&b| focused == Some(b)) {
                    return KeyAction::Activate(b);
                }
            }
            return KeyAction::None;
        }
        match key {
            Key::Escape if !repeat => {
                self.space_down = None;
                return KeyAction::Close;
            }
            Key::Tab => self.step(focus, &order, focused, if shift { -1 } else { 1 }, true),
            Key::ArrowLeft | Key::ArrowRight => {
                let dir = if key == Key::ArrowRight { 1 } else { -1 };
                self.step(focus, &order, focused, dir, p.arrows == ArrowNav::Wrap);
            }
            Key::ArrowUp | Key::ArrowDown if p.scroll_keys => {
                self.scroll += if key == Key::ArrowDown { LINE_SCROLL } else { -LINE_SCROLL };
            }
            Key::Home | Key::End if p.scroll_keys => {
                self.scroll = if key == Key::Home { -END_SCROLL } else { END_SCROLL };
            }
            Key::PageUp | Key::PageDown if p.scroll_keys => {
                let page = PAGE_FRACTION * client_h.max(0.0);
                self.scroll += if key == Key::PageDown { page } else { -page };
            }
            Key::Enter if !repeat => {
                let fallback = p.enter_falls_back_to_default && focus.is_none();
                if let Some(b) = focused.or(default_button(out).filter(|_| fallback)) {
                    return KeyAction::Activate(b);
                }
            }
            Key::Space if !repeat => match p.space {
                SpaceKey::ActivateOnPress => {
                    if let Some(b) = focused {
                        return KeyAction::Activate(b);
                    }
                }
                SpaceKey::ActivateOnRelease => self.space_down = focused,
            },
            _ => {}
        }
        KeyAction::None
    }

    /// Move focus to the next entry of `order` from `cur` in direction `dir` (±1). Nothing
    /// focused (or `cur` not in `order`): the first (forward) or last (backward) entry. Without
    /// `wrap`, focus stays put at an end. Focus becomes visible and a held Space is cancelled.
    fn step(&mut self, focus: &mut Option<usize>, order: &[usize], cur: Option<usize>, dir: isize, wrap: bool) {
        if let Some(b) = step(order, cur, dir, wrap) {
            self.focus(focus, b);
        }
    }

    /// Move focus to button `b` as keyboard navigation does (also for assistive technology focus
    /// requests): focus becomes visible and a held Space is cancelled.
    pub(crate) fn focus(&mut self, focus: &mut Option<usize>, b: usize) {
        *focus = Some(b);
        self.focus_visible = true;
        self.space_down = None;
    }

    /// This pass's `FrameInfo` (the scroll requests apply to one pass: taken).
    pub(crate) fn frame_info(&mut self, focus: Option<usize>, out: &DialogUiOutput) -> FrameInfo {
        // Space-held only shows while the button still has focus (a pointer press may move it).
        if self.space_down.is_some() && self.space_down != focused_button(focus, out) {
            self.space_down = None;
        }
        FrameInfo { focus_visible: self.focus_visible,
                    key_pressed: self.space_down,
                    scroll_request: std::mem::take(&mut self.scroll),
                    wheel_request: std::mem::take(&mut self.wheel) }
    }
}

/// The default button: the highest API index laid out.
pub(crate) fn default_button(out: &DialogUiOutput) -> Option<usize> {
    out.buttons.iter().map(|b| b.index).max()
}

/// `focus` if it is one of the last pass's buttons.
pub(crate) fn focused_button(focus: Option<usize>, out: &DialogUiOutput) -> Option<usize> {
    focus.filter(|f| out.buttons.iter().any(|b| b.index == *f))
}

/// Next entry of `order` from `cur` in direction `dir` (±1); see [`KeyboardState::step`].
fn step(order: &[usize], cur: Option<usize>, dir: isize, wrap: bool) -> Option<usize> {
    let n = order.len() as isize;
    if n == 0 {
        return None;
    }
    let Some(i) = cur.and_then(|c| order.iter().position(|&x| x == c)) else {
        return Some(order[if dir > 0 { 0 } else { order.len() - 1 }]);
    };
    let j = i as isize + dir;
    let j = if wrap { j.rem_euclid(n) } else { j };
    (0..n).contains(&j).then(|| order[j as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::draw::{Point, Rect};
    use crate::backends::fluent::KEYBOARD as FLUENT;
    use crate::backends::gui::theme::ButtonInfo;
    use crate::backends::ubuntu::KEYBOARD as UBUNTU;

    /// A row of buttons, API indices `display` from left to right.
    struct Rig {
        kb: KeyboardState,
        out: DialogUiOutput,
        focus: Option<usize>,
        shift: bool,
    }

    impl Rig {
        fn new(policy: KeyboardPolicy, display: Vec<usize>) -> Rig {
            let buttons =
                display.iter()
                       .enumerate()
                       .map(|(k, &index)| ButtonInfo { index, rect: Rect::new(10.0 + 60.0 * k as f64, 10.0, 60.0 + 60.0 * k as f64, 40.0) })
                       .collect();
            let mut r =
                Rig { kb: KeyboardState::new(policy), out: DialogUiOutput { buttons, ..Default::default() }, focus: None, shift: false };
            r.kb.on_open(&mut r.focus, &r.out);
            r
        }

        fn key(&mut self, key: Key, pressed: bool, repeat: bool) -> KeyAction {
            self.ev(Event::Key { key, pressed, repeat, shift: self.shift })
        }

        fn tap(&mut self, key: Key) -> KeyAction {
            let a = self.key(key, true, false);
            let b = self.key(key, false, false);
            if a != KeyAction::None {
                a
            } else {
                b
            }
        }

        fn ev(&mut self, ev: Event) -> KeyAction {
            self.kb.on_event(&ev, &mut self.focus, &self.out, 200.0)
        }

        fn info(&mut self) -> FrameInfo {
            self.kb.frame_info(self.focus, &self.out)
        }
    }

    fn primary(pressed: bool) -> Event {
        Event::PointerButton { pos: Point::ZERO, button: PointerButton::Primary, pressed }
    }

    #[test]
    fn open_focuses_default_and_shows_focus() {
        for (policy, display) in [(UBUNTU, vec![0, 1, 2]), (FLUENT, vec![2, 1, 0])] {
            let mut r = Rig::new(policy, display);
            assert_eq!(r.focus, Some(2));
            assert!(r.info().focus_visible);
        }
    }

    #[test]
    fn tab_wraps_both_ways() {
        let mut r = Rig::new(UBUNTU, vec![0, 1, 2]);
        r.tap(Key::Tab);
        assert_eq!(r.focus, Some(0));
        r.tap(Key::Tab);
        assert_eq!(r.focus, Some(1));
        r.shift = true;
        r.tap(Key::Tab);
        assert_eq!(r.focus, Some(0));
        r.tap(Key::Tab);
        assert_eq!(r.focus, Some(2));
        r.shift = false;
        // Repeat navigates too.
        r.key(Key::Tab, true, true);
        assert_eq!(r.focus, Some(0));
    }

    #[test]
    fn arrows_wrap_for_ubuntu_and_clamp_for_fluent() {
        let mut r = Rig::new(UBUNTU, vec![0, 1, 2]);
        r.tap(Key::ArrowRight);
        assert_eq!(r.focus, Some(0), "wraps");
        r.tap(Key::ArrowLeft);
        assert_eq!(r.focus, Some(2), "wraps back");
        r.tap(Key::ArrowUp);
        assert_eq!(r.focus, Some(2), "Up/Down don't navigate");

        // Fluent: display order reversed (API 2 leftmost).
        let mut r = Rig::new(FLUENT, vec![2, 1, 0]);
        r.tap(Key::ArrowLeft);
        assert_eq!(r.focus, Some(2), "clamped at the left end");
        r.tap(Key::ArrowRight);
        assert_eq!(r.focus, Some(1));
        r.tap(Key::ArrowRight);
        r.tap(Key::ArrowRight);
        assert_eq!(r.focus, Some(0), "clamped at the right end");
    }

    #[test]
    fn enter_and_space_activation() {
        // Linux: Enter / Space activate on press, repeat ignored.
        let mut r = Rig::new(UBUNTU, vec![0, 1]);
        assert_eq!(r.key(Key::Enter, true, false), KeyAction::Activate(1));
        assert_eq!(r.key(Key::Enter, true, true), KeyAction::None);
        assert_eq!(r.key(Key::Space, true, false), KeyAction::Activate(1));
        assert_eq!(r.key(Key::Space, false, false), KeyAction::None);
        // Nothing focused: Enter does nothing for ubuntu.
        r.focus = None;
        assert_eq!(r.key(Key::Enter, true, false), KeyAction::None);

        // Fluent: Space press shows pressed, release activates; Enter falls back to the default.
        let mut r = Rig::new(FLUENT, vec![1, 0]);
        assert_eq!(r.key(Key::Space, true, false), KeyAction::None);
        assert_eq!(r.info().key_pressed, Some(1), "key_pressed while Space is held");
        assert_eq!(r.key(Key::Space, false, false), KeyAction::Activate(1));
        assert_eq!(r.info().key_pressed, None);
        // Focus moves while Space is held: cancelled.
        r.key(Key::Space, true, false);
        r.tap(Key::Tab);
        assert_eq!(r.key(Key::Space, false, false), KeyAction::None);
        // Escape cancels a held Space and closes.
        r.key(Key::Space, true, false);
        assert_eq!(r.key(Key::Escape, true, false), KeyAction::Close);
        assert_eq!(r.key(Key::Space, false, false), KeyAction::None);
        r.focus = None;
        assert_eq!(r.key(Key::Enter, true, false), KeyAction::Activate(1));
        // Window focus loss cancels a held Space.
        r.tap(Key::Tab);
        r.key(Key::Space, true, false);
        r.ev(Event::WindowFocused(false));
        assert_eq!(r.key(Key::Space, false, false), KeyAction::None);
    }

    #[test]
    fn escape_works_without_buttons_and_ignores_repeat() {
        let mut r = Rig::new(UBUNTU, vec![]);
        assert_eq!(r.key(Key::Escape, true, true), KeyAction::None);
        assert_eq!(r.key(Key::Escape, true, false), KeyAction::Close);
        assert_eq!(r.tap(Key::Tab), KeyAction::None);
        assert_eq!(r.tap(Key::Enter), KeyAction::None);
    }

    #[test]
    fn focus_visible_modality_for_keyboard_only() {
        let mut r = Rig::new(FLUENT, vec![1, 0]);
        assert!(r.info().focus_visible);
        r.ev(Event::PointerButton { pos: Point::ZERO, button: PointerButton::Secondary, pressed: true });
        assert!(r.info().focus_visible, "only primary presses hide the focus visual");
        r.ev(primary(true));
        assert!(!r.info().focus_visible);
        r.tap(Key::Tab);
        assert!(r.info().focus_visible);
        // Linux: always visible.
        let mut r = Rig::new(UBUNTU, vec![0, 1]);
        r.ev(primary(true));
        assert!(r.info().focus_visible);
    }

    #[test]
    fn scroll_keys() {
        let mut r = Rig::new(FLUENT, vec![]);
        r.key(Key::PageDown, true, false);
        r.key(Key::ArrowUp, true, false);
        assert_eq!(r.info().scroll_request, 180.0 - 40.0);
        assert_eq!(r.info().scroll_request, 0.0, "taken");
        r.key(Key::End, true, false);
        assert_eq!(r.info().scroll_request, 1.0e6);
        // Linux: no scroll keys.
        let mut r = Rig::new(UBUNTU, vec![]);
        r.key(Key::PageDown, true, false);
        assert_eq!(r.info().scroll_request, 0.0);
        // The wheel is reported apart (applied under the pointer only).
        r.kb.wheel(30.0);
        let info = r.info();
        assert_eq!((info.scroll_request, info.wheel_request), (0.0, 30.0));
        assert_eq!(r.info().wheel_request, 0.0, "taken");
    }

    #[test]
    fn step_helper() {
        assert_eq!(step(&[0, 1, 2], None, 1, true), Some(0));
        assert_eq!(step(&[0, 1, 2], None, -1, true), Some(2));
        assert_eq!(step(&[0, 1, 2], Some(2), 1, false), None);
        assert_eq!(step(&[0, 1, 2], Some(2), 1, true), Some(0));
        assert_eq!(step(&[0, 1, 2], Some(0), -1, true), Some(2));
        assert_eq!(step(&[], None, 1, true), None);
    }
}
