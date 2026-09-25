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

use crate::backends::egui_core::a11y;
use crate::backends::egui_core::appearance::Appearance;
use crate::backends::egui_core::fonts::bundled;
use crate::backends::egui_core::text::{self, TextBlock, TextBlockWidget, TextStyle};
use crate::backends::egui_core::theme::*;

mod tokens;
mod widgets;

use tokens::*;

/// Keyboard policy (see the module docs).
pub(crate) const KEYBOARD: KeyboardPolicy = KeyboardPolicy { focus_visibility: FocusVisibility::Always,
                                                             arrows: ArrowNav::Wrap,
                                                             enter_falls_back_to_default: false,
                                                             space: SpaceKey::ActivateOnPress,
                                                             scroll_keys: false };

/// The Ubuntu theme.
pub(crate) struct UbuntuTheme {
    tokens: UbuntuTokens,
}

impl UbuntuTheme {
    pub(crate) fn new() -> Self {
        UbuntuTheme { tokens: UbuntuTokens::resolve(&Appearance::default()) }
    }
}

/// Focus-ring suppression: moving the pointer over any button hides the focused button's ring;
/// any focus move (keyboard or press) shows it again until the pointer moves.
fn focus_suppressed(ui: &Ui, button_count: usize) -> bool {
    let ctx = ui.ctx();
    let key = Id::new("ubuntu.focus_suppressed");
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

/// Whether the pointer is geometrically inside any of the first `count` buttons, callable BEFORE
/// the buttons are added this pass (egui hit-tests the current pointer against the previous
/// pass's widget rects at the start of the pass; `ctx.read_response` exposes that).
fn any_button_contains_pointer(ctx: &egui::Context, count: usize) -> bool {
    (0..count).any(|i| ctx.read_response(button_id(i)).is_some_and(|r| r.contains_pointer()))
}

impl Theme for UbuntuTheme {
    fn set_appearance(&mut self, appearance: &Appearance) {
        self.tokens = UbuntuTokens::resolve(appearance);
    }

    fn keyboard_policy(&self) -> KeyboardPolicy {
        KEYBOARD
    }

    fn icon_size(&self) -> f32 {
        ICON_SIZE
    }

    fn fonts(&self) -> ThemeFonts {
        ThemeFonts::new(bundled::UBUNTU_REGULAR, bundled::UBUNTU_BOLD)
    }

    fn configure_style(&self, style: &mut egui::Style) {
        style.visuals.panel_fill = self.tokens.bg;
    }

    fn ui(&self, view: &DialogView<'_>, ui: &mut Ui) -> DialogUiOutput {
        let tk = &self.tokens;
        let ctx = ui.ctx().clone();
        let title_style = TextStyle::bold(TITLE_SIZE, TITLE_SIZE * LINE_HEIGHT_SCALE);
        let body_style = TextStyle::regular(BODY_SIZE, BODY_SIZE * LINE_HEIGHT_SCALE);
        let origin = ui.max_rect().min;

        // The window width follows the natural (unwrapped) width of the title and body.
        let natural = text::natural_width(&ctx, view.heading, &title_style).max(text::natural_width(&ctx, view.body, &body_style));
        let win_w = window_width(natural);
        let has_icon = view.has_icon();
        let col_w = win_w - 2.0 * MARGIN - if has_icon { ICON_SIZE + MARGIN } else { 0.0 };

        // Content: the icon (top-aligned) left of a column of title / progress / body.
        let content = Rect::from_min_size(origin, Vec2::new(win_w, f32::INFINITY));
        let content = ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
                            Frame::NONE.inner_margin(MARGIN).show(ui, |ui| {
                                           ui.horizontal_top(|ui| {
                                                 if has_icon {
                                                     let (r, response) = ui.allocate_exact_size(Vec2::splat(ICON_SIZE), Sense::hover());
                                                     a11y::describe_icon(&response, view.icon);
                                                     widgets::draw_icon(ui.painter(), view, r);
                                                     ui.add_space(MARGIN);
                                                 }
                                                 ui.vertical(|ui| {
                                                       ui.set_width(col_w);
                                                       ui.spacing_mut().item_spacing.y = MARGIN;
                                                       if !view.heading.is_empty() {
                                                           let block = text::layout(&ctx, view.heading, &title_style, col_w, None);
                                                           ui.add(TextBlockWidget { block: &block, color: tk.title_text, width: col_w });
                                                       }
                                                       if let Some(progress) = view.progress {
                                                           ui.add(widgets::ProgressBar { progress, width: col_w, tk });
                                                       }
                                                       if !view.body.is_empty() {
                                                           let block = text::layout(&ctx, view.body, &body_style, col_w, None);
                                                           ui.add(TextBlockWidget { block: &block, color: tk.body_text, width: col_w });
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
        let mut out = DialogUiOutput::default();
        let mut bottom = content.bottom();
        if n > 0 {
            let labels: Vec<TextBlock> = view.buttons.iter().map(|l| text::layout(&ctx, l, &body_style, f32::INFINITY, None)).collect();
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
            // Tab order = on-screen order = API order.
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
    use crate::model::XDialogIcon;
    use crate::backends::egui_core::anim::{self, Transition};
    use egui::Pos2;
    use crate::backends::egui_core::theme::test_support::{run_ui, theme_ctx, view};

    struct Harness {
        ctx: egui::Context,
        theme: UbuntuTheme,
        /// The buttons of [`Harness::content`].
        buttons: Vec<String>,
    }

    impl Harness {
        fn new() -> Self {
            let theme = UbuntuTheme::new();
            let ctx = theme_ctx(&theme);
            Harness { ctx, theme, buttons: vec!["Cancel".to_string(), "OK".to_string()] }
        }

        /// The default content: heading, body, information icon, Cancel / OK.
        fn content(&self) -> DialogView<'_> {
            DialogView { heading: "Operation complete",
                         body: "The operation completed successfully and everything is fine.",
                         icon: &XDialogIcon::Information,
                         ..view(&self.buttons) }
        }

        fn pass(&self, t: f64, events: Vec<egui::Event>) -> DialogUiOutput {
            self.pass_with(&self.content(), t, events, false, |_| ()).0
        }

        /// One pass (`sizing`: the measure pass, as core runs it); `probe` runs inside the pass
        /// right after the theme.
        fn pass_with<R>(&self, view: &DialogView<'_>, t: f64, events: Vec<egui::Event>, sizing: bool, probe: impl FnOnce(&egui::Context) -> R) -> (DialogUiOutput, R) {
            let view = DialogView { frame: FrameInfo { focus_visible: !sizing, ..Default::default() }, ..*view };
            // The measure pass runs at the largest allowed client size.
            let screen = if sizing { Vec2::new(4096.0, 800.0) } else { Vec2::new(350.0, 200.0) };
            let raw = egui::RawInput { time: Some(t),
                                       screen_rect: Some(Rect::from_min_size(Pos2::ZERO, screen)),
                                       focused: !sizing,
                                       events,
                                       ..Default::default() };
            run_ui(&self.ctx, &self.theme, &view, raw, probe)
        }
    }

    /// Core order: measure pass, on-open focus, `reset_animations`, first real frame. The real
    /// frame builds the measured layout and shows the default button's focus look at once.
    #[test]
    fn focus_look_on_open() {
        let h = Harness::new();
        let c = h.content();
        let (measured, _) = h.pass_with(&c, 0.0, vec![], true, |_| ());
        h.ctx.memory_mut(|m| m.request_focus(button_id(1)));
        anim::reset_animations(&h.ctx);
        let focused = h.theme.tokens.focused;
        let (real, first) = h.pass_with(&c, 0.0, vec![], false, |ctx| anim::animate(ctx, button_id(1), focused, Transition::linear(0.15)));
        assert_eq!(measured, real, "measure pass must build the same layout");
        assert_eq!(first, focused);
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

    /// Hovering a button hides the focus border of the focused one; a focus move shows it again
    /// until the pointer moves.
    #[test]
    fn focus_border_suppression() {
        let h = Harness::new();
        let out = h.pass(0.0, vec![]);
        h.ctx.memory_mut(|m| m.request_focus(button_id(1)));
        let key = Id::new("ubuntu.focus_suppressed");
        let probe = |t, ev| h.pass_with(&h.content(), t, ev, false, |ctx| ctx.data(|d| d.get_temp::<(bool, Option<Id>)>(key))).1.unwrap().0;
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
}
