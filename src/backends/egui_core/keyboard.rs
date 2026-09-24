//! Core keyboard policy: keys never reach egui. This state machine moves egui
//! focus between the theme's buttons (`Memory::request_focus(button_id(i))`), decides keyboard
//! activation and Escape, and tells widgets what they need through `FrameInfo` (focus-visible
//! modality, the keyboard-pressed button, keyboard scroll requests).
//!
//! It works on the previous pass's `DialogUiOutput` (Tab order, arrow order, default button),
//! and is driven event by event, before the next pass, by `dialog.rs`.

use super::input::CoreInput;
use super::theme::{button_id, ArrowNav, DialogUiOutput, FocusVisibility, FrameInfo, KeyboardPolicy, SpaceKey};
use crate::backends::host_types::{Key, MouseButton};

/// What a key asks the dialog to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyAction {
    None,
    /// Activate the button with this API index (same path as a pointer click).
    Activate(usize),
    /// Escape: close the dialog (`WindowClosed`).
    Close,
}

/// Scroll step for Up/Down (egui's line height for the wheel), logical px.
const LINE_SCROLL: f32 = 40.0;
/// PageUp/PageDown scroll this fraction of the client height.
const PAGE_FRACTION: f32 = 0.9;
/// Home/End scroll: a large FINITE value (the ScrollArea clamps; infinity would poison its maths).
const END_SCROLL: f32 = 1.0e6;

/// Per-dialog keyboard state.
#[derive(Clone, Debug)]
pub(crate) struct KeyboardState {
    policy: KeyboardPolicy,
    focus_visible: bool,
    /// Space pressed on this button (`SpaceKey::ActivateOnRelease`), not released yet.
    space_down: Option<usize>,
    shift: bool,
    /// Accumulated keyboard scroll for the next pass.
    scroll: f32,
}

impl KeyboardState {
    pub(crate) fn new(policy: KeyboardPolicy) -> Self {
        KeyboardState { policy, focus_visible: false, space_down: None, shift: false, scroll: 0.0 }
    }

    /// The on-open step, after the measure pass: focus the default button, focus visible.
    pub(crate) fn on_open(&mut self, ctx: &egui::Context, out: &DialogUiOutput) {
        if let Some(b) = out.default_button.filter(|&b| out.buttons.iter().any(|x| x.index == b)) {
            ctx.memory_mut(|m| m.request_focus(button_id(b)));
        }
        self.focus_visible = true;
    }

    /// Process one core input event (before the next pass). `out` is the last pass's output,
    /// `client_h` the client height in logical px (page scrolling).
    pub(crate) fn on_input(&mut self, ctx: &egui::Context, ev: &CoreInput, out: &DialogUiOutput, client_h: f32) -> KeyAction {
        match *ev {
            CoreInput::Modifiers(m) => self.shift = m.shift(),
            CoreInput::Focused(false) => {
                self.space_down = None;
                self.shift = false;
            }
            CoreInput::PointerButton { button: MouseButton::Primary, pressed: true } => {
                if self.policy.focus_visibility == FocusVisibility::KeyboardOnly {
                    self.focus_visible = false;
                }
            }
            CoreInput::Key { key, pressed, repeat } => return self.on_key(ctx, key, pressed, repeat, out, client_h),
            _ => {}
        }
        KeyAction::None
    }

    fn on_key(&mut self, ctx: &egui::Context, key: Key, pressed: bool, repeat: bool, out: &DialogUiOutput, client_h: f32) -> KeyAction {
        let p = self.policy;
        let focused = focused_button(ctx, out);
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
            Key::Tab => {
                let order: Vec<usize> = out.buttons.iter().map(|b| b.index).collect();
                self.step(ctx, &order, focused, if self.shift { -1 } else { 1 }, true);
            }
            Key::ArrowLeft | Key::ArrowRight => {
                let dir = if key == Key::ArrowRight { 1 } else { -1 };
                self.step(ctx, &out.arrow_order, focused, dir, p.arrows == ArrowNav::Wrap);
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
                let fallback = p.enter_falls_back_to_default && ctx.memory(|m| m.focused()).is_none();
                if let Some(b) = focused.or(out.default_button.filter(|_| fallback)) {
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
    fn step(&mut self, ctx: &egui::Context, order: &[usize], cur: Option<usize>, dir: isize, wrap: bool) {
        if let Some(b) = step(order, cur, dir, wrap) {
            ctx.memory_mut(|m| m.request_focus(button_id(b)));
            self.focus_visible = true;
            self.space_down = None;
        }
    }

    /// This pass's `FrameInfo`. The measure pass (`sizing`) gets only the focus visibility; a real
    /// pass takes the scroll request (it applies to one pass).
    pub(crate) fn frame_info(&mut self, ctx: &egui::Context, out: &DialogUiOutput, sizing: bool) -> FrameInfo {
        if sizing {
            return FrameInfo { sizing, focus_visible: self.focus_visible, ..Default::default() };
        }
        // Space-held only shows while the button still has focus (a pointer press may move it).
        if self.space_down.is_some() && self.space_down != focused_button(ctx, out) {
            self.space_down = None;
        }
        FrameInfo { sizing, focus_visible: self.focus_visible, key_pressed: self.space_down, scroll_request: std::mem::take(&mut self.scroll) }
    }
}

/// The API index of the focused button, if egui focus is on one of the last pass's buttons.
pub(crate) fn focused_button(ctx: &egui::Context, out: &DialogUiOutput) -> Option<usize> {
    let f = ctx.memory(|m| m.focused())?;
    out.buttons.iter().map(|b| b.index).find(|&i| button_id(i) == f)
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
    use crate::backends::egui_core::theme::test_support::view;
    use crate::backends::egui_core::theme::{ButtonInteraction, DialogView};
    use crate::backends::host_types::Modifiers;
    use egui::{Pos2, Rect, Vec2};

    fn linux() -> KeyboardPolicy {
        KeyboardPolicy { focus_visibility: FocusVisibility::Always,
                         arrows: ArrowNav::Wrap,
                         enter_falls_back_to_default: false,
                         space: SpaceKey::ActivateOnPress,
                         scroll_keys: false }
    }

    fn fluent() -> KeyboardPolicy {
        KeyboardPolicy { focus_visibility: FocusVisibility::KeyboardOnly,
                         arrows: ArrowNav::Clamp,
                         enter_falls_back_to_default: true,
                         space: SpaceKey::ActivateOnRelease,
                         scroll_keys: true }
    }

    /// A stub "theme": `n` buttons in a row, Tab order = API order, arrow order = `arrow`.
    struct Rig {
        ctx: egui::Context,
        kb: KeyboardState,
        out: DialogUiOutput,
        n: usize,
        t: f64,
    }

    impl Rig {
        fn new(policy: KeyboardPolicy, n: usize, arrow: Vec<usize>) -> Rig {
            let ctx = egui::Context::default();
            ctx.options_mut(crate::backends::egui_core::theme::core_options);
            let mut r = Rig { ctx,
                              kb: KeyboardState::new(policy),
                              out: DialogUiOutput { arrow_order: arrow, default_button: n.checked_sub(1), ..Default::default() },
                              n,
                              t: 0.0 };
            r.pass(); // "measure pass": registers the buttons
            r.kb.on_open(&r.ctx.clone(), &r.out.clone());
            r.pass();
            r
        }

        /// One real egui pass with the stub buttons (so egui's focus dead-man switch is exercised).
        fn pass(&mut self) -> FrameInfo {
            self.t += 0.016;
            let frame = self.kb.frame_info(&self.ctx, &self.out, false);
            let labels: Vec<String> = (0..self.n).map(|i| i.to_string()).collect();
            let view = DialogView { frame, ..view(&labels) };
            let raw = egui::RawInput { time: Some(self.t),
                                       screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 100.0))),
                                       ..Default::default() };
            let mut out = DialogUiOutput::default();
            let mut full = self.ctx.run_ui(raw, |ui| {
                                       for i in 0..self.n {
                                           let rect = Rect::from_min_size(Pos2::new(10.0 + 60.0 * i as f32, 10.0), Vec2::new(50.0, 30.0));
                                           out.push_button(&ButtonInteraction::interact(ui, rect, i, &view));
                                       }
                                   });
            full.textures_delta.clear();
            self.out.buttons = out.buttons;
            frame
        }

        fn key(&mut self, key: Key, pressed: bool, repeat: bool) -> KeyAction {
            self.ev(CoreInput::Key { key, pressed, repeat })
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

        fn ev(&mut self, ev: CoreInput) -> KeyAction {
            let a = self.kb.on_input(&self.ctx, &ev, &self.out, 200.0);
            self.pass();
            a
        }

        fn scroll(&mut self, key: Key) {
            self.kb.on_input(&self.ctx, &CoreInput::Key { key, pressed: true, repeat: false }, &self.out, 200.0);
        }

        fn focused(&self) -> Option<usize> {
            focused_button(&self.ctx, &self.out)
        }
    }

    #[test]
    fn open_focuses_default_and_shows_focus() {
        for (policy, arrow) in [(linux(), vec![0, 1, 2]), (fluent(), vec![2, 1, 0])] {
            let mut r = Rig::new(policy, 3, arrow);
            assert_eq!(r.focused(), Some(2));
            assert!(r.pass().focus_visible);
        }
    }

    #[test]
    fn tab_wraps_both_ways() {
        let mut r = Rig::new(linux(), 3, vec![0, 1, 2]);
        r.tap(Key::Tab);
        assert_eq!(r.focused(), Some(0));
        r.tap(Key::Tab);
        assert_eq!(r.focused(), Some(1));
        r.ev(CoreInput::Modifiers(Modifiers::new(true, false, false, false)));
        r.tap(Key::Tab);
        assert_eq!(r.focused(), Some(0));
        r.tap(Key::Tab);
        assert_eq!(r.focused(), Some(2));
        r.ev(CoreInput::Modifiers(Modifiers::default()));
        // Repeat navigates too.
        r.key(Key::Tab, true, true);
        assert_eq!(r.focused(), Some(0));
    }

    #[test]
    fn arrows_wrap_for_linux_and_clamp_for_fluent() {
        let mut r = Rig::new(linux(), 3, vec![0, 1, 2]);
        r.tap(Key::ArrowRight);
        assert_eq!(r.focused(), Some(0), "wraps");
        r.tap(Key::ArrowLeft);
        assert_eq!(r.focused(), Some(2), "wraps back");
        r.tap(Key::ArrowUp);
        assert_eq!(r.focused(), Some(2), "Up/Down don't navigate");

        // Fluent: display order reversed (API 2 leftmost).
        let mut r = Rig::new(fluent(), 3, vec![2, 1, 0]);
        r.tap(Key::ArrowLeft);
        assert_eq!(r.focused(), Some(2), "clamped at the left end");
        r.tap(Key::ArrowRight);
        assert_eq!(r.focused(), Some(1));
        r.tap(Key::ArrowRight);
        r.tap(Key::ArrowRight);
        assert_eq!(r.focused(), Some(0), "clamped at the right end");
    }

    #[test]
    fn enter_and_space_activation() {
        // Linux: Enter / Space activate on press, repeat ignored.
        let mut r = Rig::new(linux(), 2, vec![0, 1]);
        assert_eq!(r.key(Key::Enter, true, false), KeyAction::Activate(1));
        assert_eq!(r.key(Key::Enter, true, true), KeyAction::None);
        assert_eq!(r.key(Key::Space, true, false), KeyAction::Activate(1));
        assert_eq!(r.key(Key::Space, false, false), KeyAction::None);
        // Nothing focused: Enter does nothing for linux.
        r.ctx.memory_mut(|m| m.surrender_focus(button_id(1)));
        assert_eq!(r.key(Key::Enter, true, false), KeyAction::None);

        // Fluent: Space press shows pressed, release activates; Enter falls back to the default.
        let mut r = Rig::new(fluent(), 2, vec![1, 0]);
        assert_eq!(r.key(Key::Space, true, false), KeyAction::None);
        assert_eq!(r.pass().key_pressed, Some(1), "key_pressed while Space is held");
        assert_eq!(r.key(Key::Space, false, false), KeyAction::Activate(1));
        assert_eq!(r.pass().key_pressed, None);
        // Focus moves while Space is held: cancelled.
        r.key(Key::Space, true, false);
        r.tap(Key::Tab);
        assert_eq!(r.key(Key::Space, false, false), KeyAction::None);
        // Escape cancels a held Space and closes.
        r.key(Key::Space, true, false);
        assert_eq!(r.key(Key::Escape, true, false), KeyAction::Close);
        assert_eq!(r.key(Key::Space, false, false), KeyAction::None);
        r.ctx.memory_mut(|m| m.surrender_focus(button_id(0)));
        r.ctx.memory_mut(|m| m.surrender_focus(button_id(1)));
        assert_eq!(r.focused(), None);
        assert_eq!(r.key(Key::Enter, true, false), KeyAction::Activate(1));
        // Window focus loss cancels a held Space.
        r.tap(Key::Tab);
        r.key(Key::Space, true, false);
        r.ev(CoreInput::Focused(false));
        assert_eq!(r.key(Key::Space, false, false), KeyAction::None);
    }

    #[test]
    fn escape_works_without_buttons_and_ignores_repeat() {
        let mut r = Rig::new(linux(), 0, vec![]);
        assert_eq!(r.key(Key::Escape, true, true), KeyAction::None);
        assert_eq!(r.key(Key::Escape, true, false), KeyAction::Close);
        assert_eq!(r.tap(Key::Tab), KeyAction::None);
        assert_eq!(r.tap(Key::Enter), KeyAction::None);
    }

    #[test]
    fn focus_visible_modality_for_keyboard_only() {
        let mut r = Rig::new(fluent(), 2, vec![1, 0]);
        assert!(r.pass().focus_visible);
        r.ev(CoreInput::PointerButton { button: MouseButton::Secondary, pressed: true });
        assert!(r.pass().focus_visible, "only primary presses hide the focus visual");
        r.ev(CoreInput::PointerButton { button: MouseButton::Primary, pressed: true });
        assert!(!r.pass().focus_visible);
        r.tap(Key::Tab);
        assert!(r.pass().focus_visible);
        // Linux: always visible.
        let mut r = Rig::new(linux(), 2, vec![0, 1]);
        r.ev(CoreInput::PointerButton { button: MouseButton::Primary, pressed: true });
        assert!(r.pass().focus_visible);
    }

    #[test]
    fn scroll_keys() {
        let mut r = Rig::new(fluent(), 0, vec![]);
        r.scroll(Key::PageDown);
        r.scroll(Key::ArrowUp);
        assert_eq!(r.kb.frame_info(&r.ctx, &r.out, false).scroll_request, 180.0 - 40.0);
        assert_eq!(r.kb.frame_info(&r.ctx, &r.out, false).scroll_request, 0.0, "taken");
        r.scroll(Key::End);
        assert_eq!(r.kb.frame_info(&r.ctx, &r.out, false).scroll_request, 1.0e6);
        // Linux: no scroll keys.
        let mut r = Rig::new(linux(), 0, vec![]);
        r.scroll(Key::PageDown);
        assert_eq!(r.kb.frame_info(&r.ctx, &r.out, false).scroll_request, 0.0);
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
