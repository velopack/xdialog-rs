//! Ubuntu theme: the look of xdialog 3.x's Linux dialogs, built from egui layout and painting.
//!
//! Layout ([`UbuntuTheme::ui`]): the window is 350-600 px wide depending on the natural text width.
//! Inside a 16 px margin, an optional 48 px icon sits left of a column of title (Ubuntu Bold 18),
//! progress bar (6 px) and body (Ubuntu Regular 14), 16 px apart. With buttons, a 48 px footer
//! holds them right-aligned, 7 px apart and 7 px from the right edge. Colours and metrics are in
//! `tokens.rs`.
//!
//! Animations: button colours fade linearly over 150 ms, the progress value tweens over 300 ms
//! OutCubic, the indeterminate capsule loops every 3 s.
//!
//! Keyboard: Tab / Left / Right move focus with wrapping, Enter / Space activate the focused
//! button on press, Escape closes. The last button is focused on open and its focus ring is drawn
//! also while the window is inactive. Hovering any button hides the focus ring until focus moves,
//! the pointer moves off the buttons or leaves the window (a hovered button then fades to idle).

use egui::{Align, Event, Frame, Id, Layout, Rect, Sense, Ui, UiBuilder, Vec2};

use crate::backends::egui_core::appearance::Appearance;
use crate::backends::egui_core::fonts::bundled;
use crate::backends::egui_core::text::{TextBlock, TextBlockWidget, TextCtx, TextStyle};
use crate::backends::egui_core::theme::*;
use crate::model::XDialogIcon;

mod icons;
mod tokens;
mod widgets;

pub(crate) use tokens::UbuntuTokens;
use tokens::*;

/// The Ubuntu theme.
pub(crate) struct UbuntuTheme;

impl UbuntuTheme {
    // Linux with neither built-in winit nor `winit-host` compiles the theme but has no entry point
    // that runs it (every dialog fails with `NoBackendAvailable`).
    #[cfg_attr(not(any(xd_own_loop, xd_winit_host, xd_test_hooks)), allow(dead_code))]
    pub(crate) fn new() -> Self {
        UbuntuTheme
    }
}

/// Focus-ring suppression: moving the pointer over any button hides the focused button's ring;
/// any focus move (keyboard or press) shows it again until the pointer moves.
fn focus_suppressed(ui: &Ui, button_count: usize) -> bool {
    let ctx = ui.ctx();
    let key = Id::new("linux.focus_suppressed");
    let focused = ctx.memory(|m| m.focused());
    let (mut suppressed, last_focus) = ctx.data(|d| d.get_temp::<(bool, Option<Id>)>(key)).unwrap_or((false, focused));
    if focused != last_focus {
        suppressed = false;
    }
    // `PointerGone` (the pointer left the window): the hovered button fades back to idle, so the
    // focus ring returns. (The button responses read here are the previous pass's, which still
    // contain the pointer.)
    let (moved, gone) = ui.input(|i| {
                              (i.events.iter().any(|e| matches!(e, Event::PointerMoved(_))), i.events.iter().any(|e| matches!(e, Event::PointerGone)))
                          });
    if gone {
        suppressed = false;
    } else if moved {
        suppressed = any_button_contains_pointer(ctx, button_count);
    }
    ctx.data_mut(|d| d.insert_temp(key, (suppressed, focused)));
    suppressed
}

impl Theme for UbuntuTheme {
    type Tokens = UbuntuTokens;

    fn keyboard_policy(&self) -> KeyboardPolicy {
        KeyboardPolicy { focus_visibility: FocusVisibility::Always,
                         arrows: ArrowNav::Wrap,
                         enter_falls_back_to_default: false,
                         space: SpaceKey::ActivateOnPress,
                         scroll_keys: false }
    }

    fn tokens(&self, appearance: &Appearance) -> UbuntuTokens {
        UbuntuTokens::resolve(appearance)
    }

    fn fonts(&self) -> ThemeFonts {
        ThemeFonts::new(bundled::ubuntu_regular(), bundled::ubuntu_bold())
    }

    fn configure_style(&self, tk: &UbuntuTokens, style: &mut egui::Style) {
        style.spacing.item_spacing = Vec2::ZERO;
        style.visuals.panel_fill = tk.bg;
        style.visuals.window_fill = tk.bg;
    }

    fn ui(&self, tk: &UbuntuTokens, view: &DialogView<'_>, ui: &mut Ui) -> DialogUiOutput {
        let ctx = ui.ctx().clone();
        let text = TextCtx::new(&ctx);
        let title_style = TextStyle::bold(TITLE_SIZE, TITLE_SIZE * LINE_HEIGHT_SCALE);
        let body_style = TextStyle::regular(BODY_SIZE, BODY_SIZE * LINE_HEIGHT_SCALE);
        let origin = ui.max_rect().min;

        // The window width follows the natural (unwrapped) width of the title and body.
        let natural = text.natural_width(view.heading, &title_style).max(text.natural_width(view.body, &body_style));
        let win_w = window_width(natural);
        let has_icon = *view.icon != XDialogIcon::None;
        let col_w = win_w - 2.0 * MARGIN - if has_icon { ICON_SIZE + MARGIN } else { 0.0 };

        // Content: the icon (top-aligned) left of a column of title / progress / body.
        let content = Rect::from_min_size(origin, Vec2::new(win_w, f32::INFINITY));
        let content = ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
                            Frame::NONE.inner_margin(MARGIN).show(ui, |ui| {
                                           ui.horizontal_top(|ui| {
                                                 if has_icon {
                                                     let (r, _) = ui.allocate_exact_size(Vec2::splat(ICON_SIZE), Sense::hover());
                                                     icons::draw_icon(ui.painter(), view.icon, r);
                                                     ui.add_space(MARGIN);
                                                 }
                                                 ui.vertical(|ui| {
                                                       ui.set_width(col_w);
                                                       ui.spacing_mut().item_spacing.y = MARGIN;
                                                       if !view.heading.is_empty() {
                                                           let block = text.layout(view.heading, &title_style, col_w, None);
                                                           ui.add(TextBlockWidget { block: &block, color: tk.title_text, width: Some(col_w) });
                                                       }
                                                       if let Some(progress) = view.progress {
                                                           ui.add(widgets::ProgressBar { progress, width: col_w, tk });
                                                       }
                                                       if !view.body.is_empty() {
                                                           let block = text.layout(view.body, &body_style, col_w, None);
                                                           ui.add(TextBlockWidget { block: &block, color: tk.body_text, width: Some(col_w) });
                                                       }
                                                   });
                                             });
                                       });
                        })
                        .response
                        .rect;

        // Footer (only with buttons): buttons right-aligned in API order; they may overflow to
        // the left.
        let n = view.buttons.len();
        let mut out = DialogUiOutput { arrow_order: (0..n).collect(), default_button: n.checked_sub(1), ..Default::default() };
        let mut bottom = content.bottom();
        if n > 0 {
            let labels: Vec<TextBlock> = view.buttons.iter().map(|l| text.layout(l, &body_style, f32::INFINITY, None)).collect();
            let focus_suppressed = focus_suppressed(ui, n);
            let footer = Rect::from_min_size(egui::pos2(origin.x, bottom), Vec2::new(win_w, FOOTER_H));
            let mut states = Vec::with_capacity(n);
            ui.scope_builder(UiBuilder::new().max_rect(footer).layout(Layout::right_to_left(Align::Center)), |ui| {
                  ui.add_space(FOOTER_MARGIN);
                  ui.spacing_mut().item_spacing.x = BUTTON_GAP;
                  for (index, label) in labels.iter().enumerate().rev() {
                      states.push(widgets::Button { index, label, view, tk, focus_suppressed }.show(ui));
                  }
              });
            // Tab order = API order.
            for st in states.iter().rev() {
                out.push_button(st);
            }
            bottom = footer.bottom();
        }
        out.desired_size = Vec2::new(win_w, bottom - origin.y);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::egui_core::anim::{self, Transition};
    use egui::Pos2;
    use crate::backends::egui_core::theme::test_support::{theme_ctx, view};

    struct Harness {
        ctx: egui::Context,
        theme: UbuntuTheme,
        tk: UbuntuTokens,
        /// `RawInput::focused` (window activation) for the next passes.
        window_focused: std::cell::Cell<bool>,
    }

    struct Content {
        heading: &'static str,
        body: &'static str,
        icon: XDialogIcon,
        buttons: Vec<String>,
        progress: Option<ProgressView>,
    }

    impl Default for Content {
        fn default() -> Self {
            Content { heading: "Operation complete",
                      body: "The operation completed successfully and everything is fine.",
                      icon: XDialogIcon::Information,
                      buttons: vec!["Cancel".to_string(), "OK".to_string()],
                      progress: None }
        }
    }

    impl Harness {
        fn new() -> Self {
            let theme = UbuntuTheme::new();
            let tk = theme.tokens(&Appearance::default());
            let ctx = theme_ctx(&theme, &tk);
            Harness { ctx, theme, tk, window_focused: std::cell::Cell::new(true) }
        }

        fn pass(&self, t: f64, events: Vec<egui::Event>) -> DialogUiOutput {
            self.pass_with(&Content::default(), t, events, false, |_| ()).0
        }

        /// One pass; `probe` runs inside the pass right after the theme.
        fn pass_with<R>(&self, c: &Content, t: f64, events: Vec<egui::Event>, sizing: bool, probe: impl FnOnce(&egui::Context) -> R) -> (DialogUiOutput, R) {
            let view = DialogView { kind: if c.progress.is_some() { DialogKind::Progress } else { DialogKind::Message },
                                    heading: c.heading,
                                    body: c.body,
                                    icon: &c.icon,
                                    progress: c.progress,
                                    frame: FrameInfo { sizing, focus_visible: true, ..Default::default() },
                                    ..view(&c.buttons) };
            // The measure pass runs at the largest allowed client size.
            let screen = if sizing { Vec2::new(4096.0, 800.0) } else { Vec2::new(350.0, 200.0) };
            let raw = egui::RawInput { time: Some(t),
                                       screen_rect: Some(Rect::from_min_size(Pos2::ZERO, screen)),
                                       focused: !sizing && self.window_focused.get(),
                                       events,
                                       ..Default::default() };
            let mut out = DialogUiOutput::default();
            let mut probe = Some(probe);
            let mut probed = None;
            let mut full = self.ctx.run_ui(raw, |ui| {
                                       out = self.theme.ui(&self.tk, &view, ui);
                                       probed = probe.take().map(|p| p(ui.ctx()));
                                   });
            full.textures_delta.clear();
            (out, probed.unwrap())
        }
    }

    /// Core order: measure pass, on-open focus, `reset_animations`, first real frame. The real
    /// frame builds the measured layout and shows the default button's focus look at once.
    #[test]
    fn focus_look_on_open() {
        let h = Harness::new();
        let c = Content::default();
        let (measured, _) = h.pass_with(&c, 0.0, vec![], true, |_| ());
        h.ctx.memory_mut(|m| m.request_focus(button_id(measured.default_button.unwrap())));
        anim::reset_animations(&h.ctx);
        let focused = h.tk.focused;
        let (real, first) = h.pass_with(&c, 0.0, vec![], false, |ctx| anim::animate(ctx, button_id(1), focused, Transition::linear(0.15)));
        assert_eq!(measured, real, "measure pass must build the same layout");
        assert_eq!(first, focused);
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() <= 2.0
    }

    #[test]
    fn layout_design_sizes() {
        let h = Harness::new();
        let out = h.pass(0.0, vec![]);
        assert_eq!(out.buttons.iter().map(|b| b.index).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(out.default_button, Some(1));
        assert_eq!(out.desired_size.x, 350.0);
        // Buttons: 7 from the right edge, 7 apart, 34 high, 7 above the window bottom.
        let (b0, b1) = (out.buttons[0].rect, out.buttons[1].rect);
        assert!(near(b1.right(), 343.0) && near(b1.left() - b0.right(), 7.0), "{:?}", out.buttons);
        assert!(near(b1.height(), 34.0) && near(out.desired_size.y - b1.bottom(), 7.0));
        // 16 + 21.6 + 16 + 2 * 16.8 + 16 + 48 = 151.2.
        assert!(near(out.desired_size.y, 151.2), "{:?}", out.desired_size);
    }

    #[test]
    fn layout_variants() {
        let h = Harness::new();
        // No icon, no heading, no buttons: 16 + 16.8 + 16.
        let c = Content { heading: "", body: "Solving string theory...", icon: XDialogIcon::None, buttons: vec![], ..Default::default() };
        let out = h.pass_with(&c, 0.0, vec![], false, |_| ()).0;
        assert!(near(out.desired_size.y, 48.8), "{:?}", out.desired_size);
        assert!(out.buttons.is_empty() && out.default_button.is_none());
        // Progress with icon: 16 + 21.6 + 16 + 6 + 16 + 16.8 + 16 = 108.4.
        let c = Content { heading: "Downloading updates",
                          body: "Downloading package 1 of 3...",
                          buttons: vec![],
                          progress: Some(ProgressView::Determinate { value: 0.0 }),
                          ..Default::default() };
        let out = h.pass_with(&c, 0.0, vec![], false, |_| ()).0;
        assert!(near(out.desired_size.y, 108.4), "{:?}", out.desired_size);
        // Icon column minimum: 80 (+ 48 footer).
        let c = Content { heading: "", body: "x", ..Default::default() };
        let out = h.pass_with(&c, 0.0, vec![], false, |_| ()).0;
        assert!(near(out.desired_size.y, 128.0), "{:?}", out.desired_size);
    }

    #[test]
    fn capsule_timeline() {
        use widgets::capsule_pos;
        assert_eq!(capsule_pos(0.0), 0.0);
        assert!((capsule_pos(0.2) - 0.5).abs() < 1e-6);
        assert_eq!(capsule_pos(0.45), 1.0);
        assert!((capsule_pos(0.7) - 0.5).abs() < 1e-6);
        assert_eq!(capsule_pos(0.95), 0.0);
    }

    #[test]
    fn pointer_click_activates_through_egui() {
        let h = Harness::new();
        let out = h.pass(0.0, vec![]);
        let c = out.buttons[0].rect.center();
        let press = |pressed| egui::Event::PointerButton { pos: c, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        assert_eq!(h.pass(0.1, vec![egui::Event::PointerMoved(c)]).activated, None);
        assert_eq!(h.pass(0.2, vec![press(true)]).activated, None);
        // The press moved egui focus to the button.
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(button_id(0)));
        // A long hold still clicks (max_click_duration = inf).
        assert_eq!(h.pass(5.0, vec![press(false)]).activated, Some(0));
        // Press, drag off, release outside: no activation.
        let off = Pos2::new(5.0, 5.0);
        h.pass(5.1, vec![press(true)]);
        let out = h.pass(5.2, vec![egui::Event::PointerMoved(off),
                                   egui::Event::PointerButton { pos: off,
                                                                button: egui::PointerButton::Primary,
                                                                pressed: false,
                                                                modifiers: Default::default() }]);
        assert_eq!(out.activated, None);
    }

    /// Hovering a button hides the focus border of the focused one; a focus move shows it again
    /// until the pointer moves.
    #[test]
    fn focus_border_suppression() {
        let h = Harness::new();
        let out = h.pass(0.0, vec![]);
        h.ctx.memory_mut(|m| m.request_focus(button_id(1)));
        let key = Id::new("linux.focus_suppressed");
        let probe = |t, ev| h.pass_with(&Content::default(), t, ev, false, |ctx| ctx.data(|d| d.get_temp::<(bool, Option<Id>)>(key))).1.unwrap().0;
        assert!(!probe(0.1, vec![]));
        let over = out.buttons[0].rect.center();
        h.pass(0.2, vec![egui::Event::PointerMoved(over)]);
        assert!(probe(0.3, vec![egui::Event::PointerMoved(over)]));
        // Keyboard focus move (core) while still hovering: visible again.
        h.ctx.memory_mut(|m| m.request_focus(button_id(0)));
        assert!(!probe(0.4, vec![]));
        assert!(probe(0.5, vec![egui::Event::PointerMoved(over + Vec2::new(1.0, 0.0))]));
        assert!(!probe(0.6, vec![egui::Event::PointerMoved(Pos2::new(5.0, 5.0))]));
        // The pointer leaves the window while over a button (core: `CursorLeft` -> `PointerGone`):
        // the hovered button fades to idle, so the focus border must come back.
        assert!(probe(0.7, vec![egui::Event::PointerMoved(over)]));
        assert!(!probe(0.8, vec![egui::Event::PointerGone]));
    }

    /// The default button keeps its focus ring while the window is inactive.
    #[test]
    fn focus_ring_kept_while_window_inactive() {
        let h = Harness::new();
        let c = Content::default();
        h.pass(0.0, vec![]);
        h.ctx.memory_mut(|m| m.request_focus(button_id(1)));
        let focused = h.tk.focused;
        let shown = |t| h.pass_with(&c, t, vec![], false, |ctx| anim::animate(ctx, button_id(1), focused, Transition::linear(0.15))).1;
        shown(1.0);
        assert_eq!(shown(1.5), focused);
        h.window_focused.set(false);
        assert_eq!(shown(1.6), focused);
        assert_eq!(shown(2.5), focused);
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(button_id(1)));
    }
}
