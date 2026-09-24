//! Core keyboard policy: keys never reach egui. This state machine moves egui
//! focus between the theme's buttons (`Memory::request_focus(button_id(i))`), decides keyboard
//! activation and Escape, and tells widgets what they need through `FrameInfo` (focus-visible
//! modality, the keyboard-pressed button, keyboard scroll requests).
//!
//! It works on the previous pass's `DialogUiOutput` (Tab order, arrow order/axis, default button),
//! and is driven event by event, before the next pass, by `dialog.rs`.

use super::input::CoreInput;
use super::theme::{button_id, ArrowAxis, ArrowNav, DialogUiOutput, FocusVisibility, KeyboardPolicy, SpaceKey};
use crate::backends::host_types::{Key, MouseButton};

/// What a key (or the end of an activation flash) asks the dialog to do.
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
    /// Keyboard activation flash: (button, clock time when the activation is delivered).
    flash: Option<(usize, f64)>,
    shift: bool,
    /// Accumulated keyboard scroll for the next pass.
    scroll: f32,
}

impl KeyboardState {
    pub(crate) fn new(policy: KeyboardPolicy) -> Self {
        KeyboardState { policy, focus_visible: false, space_down: None, flash: None, shift: false, scroll: 0.0 }
    }

    /// The on-open step, after the measure pass: focus the default button (unless disabled) and
    /// set the initial focus visibility.
    pub(crate) fn on_open(&mut self, ctx: &egui::Context, out: &DialogUiOutput, disabled: &[bool]) {
        if self.policy.focus_on_open {
            if let Some(b) = out.default_button.filter(|&b| !is_disabled(disabled, b) && out.buttons.iter().any(|x| x.index == b)) {
                ctx.memory_mut(|m| m.request_focus(button_id(b)));
            }
        }
        self.focus_visible = match self.policy.focus_visibility {
            FocusVisibility::Always => true,
            FocusVisibility::KeyboardOnly { on_open } => on_open,
        };
    }

    /// Process one core input event (before the next pass). `out` is the last pass's output,
    /// `now` the dialog clock, `client_h` the client height in logical px (page scrolling).
    pub(crate) fn on_input(&mut self,
                           ctx: &egui::Context,
                           ev: &CoreInput,
                           out: &DialogUiOutput,
                           disabled: &[bool],
                           now: f64,
                           client_h: f32)
                           -> KeyAction {
        match *ev {
            CoreInput::Modifiers(m) => {
                self.shift = m.shift();
                KeyAction::None
            }
            CoreInput::Focused(false) | CoreInput::PointerCancel => {
                if matches!(ev, CoreInput::Focused(false)) {
                    self.space_down = None;
                    self.flash = None;
                    self.shift = false;
                }
                KeyAction::None
            }
            CoreInput::PointerButton { button: MouseButton::Primary, pressed: true } => {
                if matches!(self.policy.focus_visibility, FocusVisibility::KeyboardOnly { .. }) {
                    self.focus_visible = false;
                }
                KeyAction::None
            }
            CoreInput::Key { key, pressed, repeat } => self.on_key(ctx, key, pressed, repeat, out, disabled, now, client_h),
            _ => KeyAction::None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn on_key(&mut self,
              ctx: &egui::Context,
              key: Key,
              pressed: bool,
              repeat: bool,
              out: &DialogUiOutput,
              disabled: &[bool],
              now: f64,
              client_h: f32)
              -> KeyAction {
        let p = self.policy;
        if !pressed {
            // Only Space-release matters.
            if key == Key::Space && p.space == SpaceKey::ActivateOnRelease {
                if let Some(b) = self.space_down.take() {
                    if focused_button(ctx, out) == Some(b) && !is_disabled(disabled, b) {
                        return self.activate(b, now);
                    }
                }
            }
            return KeyAction::None;
        }
        let nav_ok = !repeat || p.nav_repeat;
        match key {
            Key::Escape => {
                if repeat || !p.escape_closes {
                    return KeyAction::None;
                }
                self.space_down = None;
                self.flash = None;
                KeyAction::Close
            }
            Key::Tab => {
                if p.tab && nav_ok {
                    let order: Vec<usize> = out.buttons.iter().map(|b| b.index).collect();
                    let dir = if self.shift { -1 } else { 1 };
                    if let Some(b) = step(&order, focused_button(ctx, out), dir, true, disabled) {
                        self.focus(ctx, b);
                    }
                }
                KeyAction::None
            }
            Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
                let horizontal = matches!(key, Key::ArrowLeft | Key::ArrowRight);
                let along = match out.arrow_axis {
                    ArrowAxis::Horizontal => horizontal,
                    ArrowAxis::Vertical => !horizontal,
                };
                let forward = matches!(key, Key::ArrowRight | Key::ArrowDown);
                if along && p.arrows != ArrowNav::Off && !out.buttons.is_empty() {
                    if nav_ok {
                        let wrap = p.arrows == ArrowNav::Wrap;
                        if let Some(b) = step(&out.arrow_order, focused_button(ctx, out), if forward { 1 } else { -1 }, wrap, disabled) {
                            self.focus(ctx, b);
                        }
                    }
                } else if !horizontal && p.scroll_keys {
                    // Up/Down that don't navigate (no buttons, or a horizontal button row) scroll.
                    self.scroll += if forward { LINE_SCROLL } else { -LINE_SCROLL };
                }
                KeyAction::None
            }
            Key::Home | Key::End => {
                let first = key == Key::Home;
                if p.home_end && !out.arrow_order.is_empty() {
                    if nav_ok {
                        let enabled = |b: &&usize| !is_disabled(disabled, **b);
                        let pick = if first { out.arrow_order.iter().find(enabled) } else { out.arrow_order.iter().rev().find(enabled) };
                        if let Some(&b) = pick {
                            self.focus(ctx, b);
                        }
                    }
                } else if p.scroll_keys {
                    self.scroll = if first { -END_SCROLL } else { END_SCROLL };
                }
                KeyAction::None
            }
            Key::PageUp | Key::PageDown => {
                if p.scroll_keys {
                    let page = PAGE_FRACTION * client_h.max(0.0);
                    self.scroll += if key == Key::PageDown { page } else { -page };
                }
                KeyAction::None
            }
            Key::Enter => {
                if repeat || !p.enter {
                    return KeyAction::None;
                }
                let target = focused_button(ctx, out).or_else(|| {
                                                         if p.enter_falls_back_to_default && ctx.memory(|m| m.focused()).is_none() {
                                                             out.default_button
                                                         } else {
                                                             None
                                                         }
                                                     });
                match target {
                    Some(b) if !is_disabled(disabled, b) => self.activate(b, now),
                    _ => KeyAction::None,
                }
            }
            Key::Space => {
                if repeat {
                    return KeyAction::None;
                }
                match p.space {
                    SpaceKey::Ignore => KeyAction::None,
                    SpaceKey::ActivateOnPress => match focused_button(ctx, out) {
                        Some(b) if !is_disabled(disabled, b) => self.activate(b, now),
                        _ => KeyAction::None,
                    },
                    SpaceKey::ActivateOnRelease => {
                        self.space_down = focused_button(ctx, out).filter(|&b| !is_disabled(disabled, b));
                        KeyAction::None
                    }
                }
            }
        }
    }

    /// Move egui focus to button `b` (keyboard navigation): focus becomes visible, Space-held is
    /// cancelled.
    fn focus(&mut self, ctx: &egui::Context, b: usize) {
        ctx.memory_mut(|m| m.request_focus(button_id(b)));
        self.focus_visible = true;
        self.space_down = None;
    }

    fn activate(&mut self, b: usize, now: f64) -> KeyAction {
        if self.flash.is_some() {
            return KeyAction::None; // one activation at a time
        }
        match self.policy.activate_flash {
            Some(d) if !d.is_zero() => {
                self.flash = Some((b, now + d.as_secs_f64()));
                KeyAction::None
            }
            _ => KeyAction::Activate(b),
        }
    }

    /// Deliver a pending flash activation whose time has come.
    pub(crate) fn poll(&mut self, now: f64) -> KeyAction {
        match self.flash {
            Some((b, until)) if now >= until => {
                self.flash = None;
                KeyAction::Activate(b)
            }
            _ => KeyAction::None,
        }
    }

    /// `(focus_visible, key_pressed, scroll_request)` for this pass's `FrameInfo`; the scroll
    /// request is taken (it applies to one pass).
    pub(crate) fn frame_info_parts(&mut self, ctx: &egui::Context, out: &DialogUiOutput) -> (bool, Option<usize>, f32) {
        let focused = focused_button(ctx, out);
        // Space-held only shows while the button still has focus (a pointer press may move it).
        if self.space_down.is_some() && self.space_down != focused {
            self.space_down = None;
        }
        let key_pressed = self.flash.map(|(b, _)| b).or(self.space_down);
        let scroll = std::mem::take(&mut self.scroll);
        (self.focus_visible, key_pressed, scroll)
    }

    /// Clock time of the pending flash activation, if any.
    pub(crate) fn next_deadline(&self) -> Option<f64> {
        self.flash.map(|(_, until)| until)
    }

    pub(crate) fn focus_visible(&self) -> bool {
        self.focus_visible
    }
}

fn is_disabled(disabled: &[bool], b: usize) -> bool {
    disabled.get(b).copied().unwrap_or(false)
}

/// The API index of the focused button, if egui focus is on one of the last pass's buttons.
pub(crate) fn focused_button(ctx: &egui::Context, out: &DialogUiOutput) -> Option<usize> {
    let f = ctx.memory(|m| m.focused())?;
    out.buttons.iter().map(|b| b.index).find(|&i| button_id(i) == f)
}

/// Next enabled entry of `order` from `cur` in direction `dir` (±1). Nothing focused (or `cur` not
/// in `order`): the first (forward) or last (backward) enabled entry. `wrap = false` clamps: at an
/// end, stays put.
fn step(order: &[usize], cur: Option<usize>, dir: isize, wrap: bool, disabled: &[bool]) -> Option<usize> {
    let n = order.len() as isize;
    if n == 0 {
        return None;
    }
    let enabled = |i: isize| !is_disabled(disabled, order[i as usize]);
    let start = cur.and_then(|c| order.iter().position(|&x| x == c));
    let Some(start) = start else {
        let mut i = if dir > 0 { 0 } else { n - 1 };
        while (0..n).contains(&i) {
            if enabled(i) {
                return Some(order[i as usize]);
            }
            i += dir;
        }
        return None;
    };
    let mut i = start as isize;
    for _ in 0..n {
        i += dir;
        if wrap {
            i = i.rem_euclid(n);
        } else if !(0..n).contains(&i) {
            return None;
        }
        if enabled(i) {
            return Some(order[i as usize]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::egui_core::theme::{ButtonInfo, ButtonInteraction, DialogKind, DialogView, FrameInfo, Platform, SizeLimits, ThemeEnv};
    use crate::backends::host_types::Modifiers;
    use crate::model::XDialogIcon;
    use egui::{Pos2, Rect, Vec2};
    use std::time::Duration;

    fn linux() -> KeyboardPolicy {
        KeyboardPolicy { focus_on_open: true,
                         focus_visibility: FocusVisibility::Always,
                         tab: true,
                         arrows: ArrowNav::Wrap,
                         nav_repeat: true,
                         home_end: false,
                         enter: true,
                         enter_falls_back_to_default: false,
                         space: SpaceKey::ActivateOnPress,
                         activate_flash: None,
                         escape_closes: true,
                         scroll_keys: false }
    }

    fn fluent() -> KeyboardPolicy {
        KeyboardPolicy { focus_visibility: FocusVisibility::KeyboardOnly { on_open: true },
                         arrows: ArrowNav::Clamp,
                         enter_falls_back_to_default: true,
                         space: SpaceKey::ActivateOnRelease,
                         scroll_keys: true,
                         ..linux() }
    }

    /// A stub "theme": `n` buttons in a row, Tab order = API order, arrow order = `arrow`.
    struct Rig {
        ctx: egui::Context,
        kb: KeyboardState,
        out: DialogUiOutput,
        disabled: Vec<bool>,
        t: f64,
    }

    impl Rig {
        fn new(policy: KeyboardPolicy, n: usize, arrow: Vec<usize>, axis: ArrowAxis) -> Rig {
            let ctx = egui::Context::default();
            ctx.options_mut(crate::backends::egui_core::theme::core_options);
            let mut r = Rig { ctx,
                              kb: KeyboardState::new(policy),
                              out: DialogUiOutput { arrow_order: arrow, arrow_axis: axis, default_button: n.checked_sub(1), ..Default::default() },
                              disabled: vec![false; n],
                              t: 0.0 };
            r.pass(); // "measure pass": registers the buttons
            r.kb.on_open(&r.ctx.clone(), &r.out.clone(), &r.disabled.clone());
            r.pass();
            r
        }

        /// One real egui pass with the stub buttons (so egui's focus dead-man switch is exercised).
        fn pass(&mut self) -> (bool, Option<usize>, f32) {
            self.t += 0.016;
            let parts = self.kb.frame_info_parts(&self.ctx, &self.out);
            let env = ThemeEnv { appearance: Default::default(), platform: Platform::current() };
            let n = self.disabled.len();
            let labels: Vec<String> = (0..n).map(|i| i.to_string()).collect();
            let icon = XDialogIcon::None;
            let view = DialogView { kind: DialogKind::Message,
                                    title: "",
                                    heading: "",
                                    body: "",
                                    icon: &icon,
                                    buttons: &labels,
                                    disabled: &self.disabled,
                                    progress: None,
                                    env: &env,
                                    limits: SizeLimits { max_height: 800.0, max_width: 800.0 },
                                    frame: FrameInfo { time: self.t,
                                                       ppp: 1.0,
                                                       sizing: false,
                                                       window_focused: true,
                                                       focus_visible: parts.0,
                                                       key_pressed: parts.1,
                                                       scroll_request: parts.2 } };
            let raw = egui::RawInput { time: Some(self.t),
                                       screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 100.0))),
                                       ..Default::default() };
            let mut buttons = Vec::new();
            let mut full = self.ctx.run_ui(raw, |ui| {
                                       for i in 0..n {
                                           let rect = Rect::from_min_size(Pos2::new(10.0 + 60.0 * i as f32, 10.0), Vec2::new(50.0, 30.0));
                                           ButtonInteraction::interact(ui, rect, i, &view);
                                           buttons.push(ButtonInfo { index: i, rect });
                                       }
                                   });
            full.textures_delta.clear();
            self.out.buttons = buttons;
            parts
        }

        fn key(&mut self, key: Key, pressed: bool, repeat: bool) -> KeyAction {
            let ev = CoreInput::Key { key, pressed, repeat };
            let a = self.kb.on_input(&self.ctx, &ev, &self.out, &self.disabled, self.t, 200.0);
            self.pass();
            a
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
            let a = self.kb.on_input(&self.ctx, &ev, &self.out, &self.disabled, self.t, 200.0);
            self.pass();
            a
        }

        fn focused(&self) -> Option<usize> {
            focused_button(&self.ctx, &self.out)
        }
    }

    #[test]
    fn open_focuses_default_and_visibility_per_policy() {
        let r = Rig::new(linux(), 3, vec![0, 1, 2], ArrowAxis::Horizontal);
        assert_eq!(r.focused(), Some(2));
        assert!(r.kb.focus_visible());
        let r = Rig::new(fluent(), 3, vec![2, 1, 0], ArrowAxis::Horizontal);
        assert_eq!(r.focused(), Some(2));
        assert!(r.kb.focus_visible());
        let mut p = fluent();
        p.focus_visibility = FocusVisibility::KeyboardOnly { on_open: false };
        let r = Rig::new(p, 2, vec![0, 1], ArrowAxis::Horizontal);
        assert!(!r.kb.focus_visible());
        let mut p = linux();
        p.focus_on_open = false;
        let r = Rig::new(p, 2, vec![0, 1], ArrowAxis::Horizontal);
        assert_eq!(r.focused(), None);
    }

    #[test]
    fn tab_wraps_both_ways_and_skips_disabled() {
        let mut r = Rig::new(linux(), 3, vec![0, 1, 2], ArrowAxis::Horizontal);
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
        r.disabled[0] = true;
        r.tap(Key::Tab);
        assert_eq!(r.focused(), Some(1));
        // Repeat navigates too (nav_repeat).
        r.key(Key::Tab, true, true);
        assert_eq!(r.focused(), Some(2));
    }

    #[test]
    fn arrows_wrap_for_linux_and_clamp_for_fluent() {
        let mut r = Rig::new(linux(), 3, vec![0, 1, 2], ArrowAxis::Horizontal);
        r.tap(Key::ArrowRight);
        assert_eq!(r.focused(), Some(0), "wraps");
        r.tap(Key::ArrowLeft);
        assert_eq!(r.focused(), Some(2), "wraps back");
        r.tap(Key::ArrowUp);
        assert_eq!(r.focused(), Some(2), "cross axis ignored");

        // Fluent: display order reversed (API 2 leftmost).
        let mut r = Rig::new(fluent(), 3, vec![2, 1, 0], ArrowAxis::Horizontal);
        r.tap(Key::ArrowLeft);
        assert_eq!(r.focused(), Some(2), "clamped at the left end");
        r.tap(Key::ArrowRight);
        assert_eq!(r.focused(), Some(1));
        r.tap(Key::ArrowRight);
        r.tap(Key::ArrowRight);
        assert_eq!(r.focused(), Some(0), "clamped at the right end");

        // Stacked buttons navigate with Up/Down.
        let mut r = Rig::new(linux(), 2, vec![1, 0], ArrowAxis::Vertical);
        r.tap(Key::ArrowDown);
        assert_eq!(r.focused(), Some(0));
        r.tap(Key::ArrowRight);
        assert_eq!(r.focused(), Some(0));
    }

    #[test]
    fn enter_and_space_activation() {
        // Linux: Enter / Space activate on press, repeat ignored.
        let mut r = Rig::new(linux(), 2, vec![0, 1], ArrowAxis::Horizontal);
        assert_eq!(r.key(Key::Enter, true, false), KeyAction::Activate(1));
        assert_eq!(r.key(Key::Enter, true, true), KeyAction::None);
        assert_eq!(r.key(Key::Space, true, false), KeyAction::Activate(1));
        assert_eq!(r.key(Key::Space, false, false), KeyAction::None);
        // Nothing focused: Enter does nothing for linux.
        r.ctx.memory_mut(|m| m.surrender_focus(button_id(1)));
        assert_eq!(r.key(Key::Enter, true, false), KeyAction::None);

        // Fluent: Space press shows pressed, release activates; Enter falls back to the default.
        let mut r = Rig::new(fluent(), 2, vec![1, 0], ArrowAxis::Horizontal);
        assert_eq!(r.key(Key::Space, true, false), KeyAction::None);
        assert_eq!(r.pass().1, Some(1), "key_pressed while Space is held");
        assert_eq!(r.key(Key::Space, false, false), KeyAction::Activate(1));
        assert_eq!(r.pass().1, None);
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
        let mut r = Rig::new(linux(), 0, vec![], ArrowAxis::Horizontal);
        assert_eq!(r.key(Key::Escape, true, true), KeyAction::None);
        assert_eq!(r.key(Key::Escape, true, false), KeyAction::Close);
        assert_eq!(r.tap(Key::Tab), KeyAction::None);
        assert_eq!(r.tap(Key::Enter), KeyAction::None);
    }

    #[test]
    fn focus_visible_modality_for_keyboard_only() {
        let mut r = Rig::new(fluent(), 2, vec![1, 0], ArrowAxis::Horizontal);
        assert!(r.pass().0);
        r.ev(CoreInput::PointerButton { button: MouseButton::Secondary, pressed: true });
        assert!(r.pass().0, "only primary presses hide the focus visual");
        r.ev(CoreInput::PointerButton { button: MouseButton::Primary, pressed: true });
        assert!(!r.pass().0);
        r.tap(Key::Tab);
        assert!(r.pass().0);
        // Linux: always visible.
        let mut r = Rig::new(linux(), 2, vec![0, 1], ArrowAxis::Horizontal);
        r.ev(CoreInput::PointerButton { button: MouseButton::Primary, pressed: true });
        assert!(r.pass().0);
    }

    #[test]
    fn scroll_keys() {
        let mut r = Rig::new(fluent(), 0, vec![], ArrowAxis::Horizontal);
        r.kb.on_input(&r.ctx, &CoreInput::Key { key: Key::PageDown, pressed: true, repeat: false }, &r.out, &[], 0.0, 200.0);
        r.kb.on_input(&r.ctx, &CoreInput::Key { key: Key::ArrowUp, pressed: true, repeat: false }, &r.out, &[], 0.0, 200.0);
        assert_eq!(r.kb.frame_info_parts(&r.ctx, &r.out).2, 180.0 - 40.0);
        assert_eq!(r.kb.frame_info_parts(&r.ctx, &r.out).2, 0.0, "taken");
        r.kb.on_input(&r.ctx, &CoreInput::Key { key: Key::End, pressed: true, repeat: false }, &r.out, &[], 0.0, 200.0);
        assert_eq!(r.kb.frame_info_parts(&r.ctx, &r.out).2, 1.0e6);
        // Linux: no scroll keys.
        let mut r = Rig::new(linux(), 0, vec![], ArrowAxis::Horizontal);
        r.kb.on_input(&r.ctx, &CoreInput::Key { key: Key::PageDown, pressed: true, repeat: false }, &r.out, &[], 0.0, 200.0);
        assert_eq!(r.kb.frame_info_parts(&r.ctx, &r.out).2, 0.0);
    }

    #[test]
    fn activation_flash_delays_delivery() {
        let mut p = linux();
        p.activate_flash = Some(Duration::from_millis(250));
        let mut r = Rig::new(p, 2, vec![0, 1], ArrowAxis::Horizontal);
        let t0 = r.t;
        assert_eq!(r.key(Key::Enter, true, false), KeyAction::None);
        assert_eq!(r.kb.next_deadline(), Some(t0 + 0.25));
        assert_eq!(r.pass().1, Some(1));
        assert_eq!(r.kb.poll(t0 + 0.1), KeyAction::None);
        assert_eq!(r.kb.poll(t0 + 0.25), KeyAction::Activate(1));
        assert_eq!(r.kb.next_deadline(), None);
    }

    #[test]
    fn step_helper() {
        assert_eq!(step(&[0, 1, 2], None, 1, true, &[]), Some(0));
        assert_eq!(step(&[0, 1, 2], None, -1, true, &[]), Some(2));
        assert_eq!(step(&[0, 1, 2], Some(2), 1, false, &[]), None);
        assert_eq!(step(&[0, 1, 2], Some(1), 1, false, &[false, false, true]), None);
        assert_eq!(step(&[0, 1, 2], Some(0), 1, true, &[false, true, false]), Some(2));
        assert_eq!(step(&[0, 1], Some(0), 1, true, &[true, true]), None);
    }
}
