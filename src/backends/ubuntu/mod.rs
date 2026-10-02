//! Ubuntu theme: the look of xdialog 3.x's Linux dialogs.
//!
//! Layout ([`UbuntuTheme::ui`]): the window is 350-600 px wide depending on the natural text width.
//! Inside a 16 px margin, an optional 48 px icon (a severity icon, or the custom image) sits left
//! of a column of title (Ubuntu Bold 18), progress bar (6 px) and body (Ubuntu Regular 14), 16 px
//! apart. With buttons, a 48 px footer holds them right-aligned, 7 px apart and 7 px from the
//! right edge. Colours and metrics are in `tokens.rs`.
//!
//! Animations: button colours fade linearly over 150 ms, the progress value tweens over 300 ms
//! OutCubic, the indeterminate capsule loops every 3 s.
//!
//! Keyboard: Tab / Left / Right move focus with wrapping, Enter / Space activate the focused
//! button on press, Escape closes. The last button is focused on open and its focus ring is drawn
//! also while the window is inactive. Hovering any button hides the focus ring until focus moves,
//! the pointer moves off the buttons or leaves the window (a hovered button then fades to idle).

use crate::backends::draw::{Color, Point, Rect, Size};
use crate::backends::gui::appearance::Appearance;
use crate::backends::gui::text::TextStyle;
use crate::backends::gui::theme::*;
use crate::backends::gui::ui::Column;

mod tokens;
mod widgets;

pub(crate) use tokens::FONTS;
use tokens::*;

/// Keyboard policy (see the module docs).
pub(crate) const KEYBOARD: KeyboardPolicy = KeyboardPolicy { focus_visibility: FocusVisibility::Always,
                                                             arrows: ArrowNav::Wrap,
                                                             enter_falls_back_to_default: false,
                                                             enter_activates_default: false,
                                                             space: SpaceKey::ActivateOnPress,
                                                             scroll_keys: false };

/// The Ubuntu theme.
pub(crate) struct UbuntuTheme {
    tokens: UbuntuTokens,
    /// The focus ring is hidden (the pointer moved over a button) ...
    focus_suppressed: bool,
    /// ... until focus moves away from this button.
    suppressed_focus: Option<usize>,
}

impl UbuntuTheme {
    pub(crate) fn new() -> Self {
        UbuntuTheme { tokens: UbuntuTokens::resolve(&Appearance::default()), focus_suppressed: false, suppressed_focus: None }
    }

    /// Focus-ring suppression: moving the pointer over any button hides the focused button's ring;
    /// any focus move (keyboard or press) or the pointer leaving the window shows it again.
    fn update_focus_suppression(&mut self, ui: &Ui<'_>, buttons: &[Rect]) {
        let focused = ui.focused_button();
        if focused != self.suppressed_focus {
            self.focus_suppressed = false;
        }
        if ui.pointer_gone() {
            self.focus_suppressed = false;
        } else if ui.pointer_moved() {
            self.focus_suppressed = ui.pointer().is_some_and(|p| buttons.iter().any(|r| r.contains(p)));
        }
        self.suppressed_focus = focused;
    }
}

impl Theme for UbuntuTheme {
    fn set_appearance(&mut self, appearance: &Appearance) {
        self.tokens = UbuntuTokens::resolve(appearance);
    }

    fn keyboard_policy(&self) -> KeyboardPolicy {
        KEYBOARD
    }

    fn fonts(&self) -> &'static ThemeFonts {
        &FONTS
    }

    fn icon_size(&self) -> f64 {
        ICON_SIZE
    }

    fn clear_color(&self) -> Color {
        self.tokens.bg
    }

    fn ui(&mut self, view: &DialogView<'_>, ui: &mut Ui<'_>) -> DialogUiOutput {
        let tk = self.tokens.clone();
        let title_style = TextStyle::bold(TITLE_SIZE);
        let body_style = TextStyle::regular(BODY_SIZE);
        let mut out = DialogUiOutput::default();

        // The window width follows the natural (unwrapped) width of the title and body.
        let title_w = ui.layout(view.heading, &title_style, f64::INFINITY, None).size.width;
        let body_w = ui.layout(view.body, &body_style, f64::INFINITY, None).size.width;
        let win_w = window_width(title_w.max(body_w));
        let has_icon = view.has_icon();
        let x0 = MARGIN + if has_icon { ICON_SIZE + MARGIN } else { 0.0 };
        let col_w = win_w - MARGIN - x0;

        // Content: the icon (top-aligned) left of a column of title / progress / body, 16 apart.
        let mut col = Column::new(x0, x0 + col_w, MARGIN);
        let mut first = true;
        let mut next = |col: &mut Column, h: f64| {
            if !std::mem::take(&mut first) {
                col.space(MARGIN);
            }
            col.row(h)
        };
        if has_icon {
            let r = Rect::from_origin_size(Point::new(MARGIN, MARGIN), Size::new(ICON_SIZE, ICON_SIZE));
            widgets::icon(ui, view, r);
            out.parts.icon = Some(r);
        }
        if !view.heading.is_empty() {
            let block = ui.layout(view.heading, &title_style, col_w, None);
            let r = next(&mut col, block.size.height);
            ui.text_in(&block, r.origin(), col_w, tk.title_text);
            out.parts.heading = Some(r);
        }
        if let Some(progress) = view.progress {
            let r = next(&mut col, PROGRESS_H);
            widgets::progress(ui, r, progress, &tk);
            out.parts.progress = Some(r);
        }
        if !view.body.is_empty() {
            let block = ui.layout(view.body, &body_style, col_w, None);
            let r = next(&mut col, block.size.height);
            ui.text_in(&block, r.origin(), col_w, tk.body_text);
            out.parts.body = Some(r);
        }
        let row_h = (col.y - MARGIN).max(if has_icon { ICON_SIZE } else { 0.0 });
        let mut bottom = MARGIN + row_h + MARGIN;

        // Footer (only with buttons): buttons right-aligned in API order; they may overflow to
        // the left.
        let n = view.buttons.len();
        if n > 0 {
            let labels: Vec<_> = view.buttons.iter().map(|l| ui.layout(l, &body_style, f64::INFINITY, None)).collect();
            let mut rects = vec![Rect::ZERO; n];
            let mut right = win_w - FOOTER_MARGIN;
            for i in (0..n).rev() {
                let w = widgets::button_width(&labels[i]);
                rects[i] = Rect::from_origin_size(Point::new(right - w, bottom + FOOTER_MARGIN), Size::new(w, BUTTON_H));
                right -= w + BUTTON_GAP;
            }
            self.update_focus_suppression(ui, &rects);
            // Tab order = on-screen order = API order.
            for (i, label) in labels.iter().enumerate() {
                let st = widgets::button(ui, rects[i], i, label, view, &tk, self.focus_suppressed);
                out.push_button(&st);
            }
            bottom += FOOTER_H;
        }
        out.desired_size = Size::new(win_w, bottom);
        out
    }
}

#[cfg(all(test, draw_soft))]
mod tests {
    use super::*;
    use crate::backends::draw::Vec2;
    use crate::backends::gui::input::Event;
    use crate::backends::gui::theme::test_support::{pass_with, state, view};
    use crate::backends::gui::ui::UiState;
    use crate::model::XDialogIcon;

    fn content(buttons: &[String]) -> DialogView<'_> {
        DialogView { heading: "Operation complete",
                     body: "The operation completed successfully and everything is fine.",
                     icon: &XDialogIcon::Information,
                     frame: FrameInfo { focus_visible: true, ..Default::default() },
                     ..view(buttons) }
    }

    #[test]
    fn layout_metrics() {
        let buttons = vec!["Cancel".to_string(), "OK".to_string()];
        let mut theme = UbuntuTheme::new();
        let (out, _) = pass_with(&mut theme, &content(&buttons), &mut state(), 0.0);
        assert_eq!(out.desired_size.width, 350.0);
        let (cancel, ok) = (out.buttons[0].rect, out.buttons[1].rect);
        assert_eq!(ok.x1, 350.0 - FOOTER_MARGIN);
        assert_eq!(ok.x0 - cancel.x1, BUTTON_GAP);
        assert_eq!((ok.height(), ok.y1 + FOOTER_MARGIN), (BUTTON_H, out.desired_size.height));
        assert_eq!(out.parts.icon, Some(Rect::new(16.0, 16.0, 64.0, 64.0)));
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
    /// until the pointer moves; leaving the window shows it again.
    #[test]
    fn focus_border_suppression() {
        let buttons = vec!["Cancel".to_string(), "OK".to_string()];
        let mut theme = UbuntuTheme::new();
        let mut st = state();
        st.focus = Some(1);
        let (out, _) = pass_with(&mut theme, &content(&buttons), &mut st, 0.0);
        let over = out.buttons[0].rect.center();
        let mut step = |st: &mut UiState, ev: Option<Event>| {
            match ev {
                Some(Event::PointerMoved(p)) => {
                    st.pointer = Some(p);
                    st.moved = true;
                }
                Some(Event::PointerGone) => {
                    st.pointer = None;
                    st.gone = true;
                }
                _ => {}
            }
            pass_with(&mut theme, &content(&buttons), st, 0.0);
            theme.focus_suppressed
        };
        assert!(!step(&mut st, None));
        assert!(step(&mut st, Some(Event::PointerMoved(over))));
        // Keyboard focus move (core) while still hovering: visible again.
        st.focus = Some(0);
        assert!(!step(&mut st, None));
        assert!(step(&mut st, Some(Event::PointerMoved(over + Vec2::new(1.0, 0.0)))));
        assert!(!step(&mut st, Some(Event::PointerMoved(Point::new(5.0, 5.0)))));
        assert!(step(&mut st, Some(Event::PointerMoved(over))));
        assert!(!step(&mut st, Some(Event::PointerGone)));
    }
}
