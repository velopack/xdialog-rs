//! Ubuntu theme: the look of xdialog 3.x's Linux dialogs.
//!
//! Layout ([`UbuntuTheme::ui`]): the window is 350-600 px wide depending on the natural text width,
//! or wider (up to 600) to fit the buttons, whose labels are elided beyond that. Inside a 16 px
//! margin, an optional 48 px icon (a severity icon, or the custom image) sits left of a column of
//! title (Ubuntu Bold 18), progress bar (6 px) and body (Ubuntu Regular 14), 16 px apart; a body
//! too tall for the height limit scrolls. With buttons, a 48 px footer holds them right-aligned,
//! 7 px apart and 7 px from the right edge. Colours and metrics are in `tokens.rs`.
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
    /// The focus ring is hidden (the pointer moved over a button).
    focus_suppressed: bool,
    /// The focused button at the last pass.
    last_focus: Option<usize>,
    /// Body scroll offset, logical px.
    scroll: f64,
}

impl UbuntuTheme {
    pub(crate) fn new() -> Self {
        UbuntuTheme { tokens: UbuntuTokens::resolve(&Appearance::default()), focus_suppressed: false, last_focus: None, scroll: 0.0 }
    }

    /// Focus-ring suppression: moving the pointer over any button hides the focused button's ring;
    /// any focus move (keyboard or press) or the pointer leaving the window shows it again.
    fn update_focus_suppression(&mut self, ui: &Ui<'_>, buttons: &[Rect]) {
        let focused = ui.focused_button();
        let focus_moved = focused != self.last_focus;
        self.focus_suppressed = if ui.pointer_gone() {
            false
        } else if ui.pointer_moved() {
            ui.pointer().is_some_and(|p| buttons.iter().any(|r| r.contains(p)))
        } else {
            self.focus_suppressed && !focus_moved
        };
        self.last_focus = focused;
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

        // The window width follows the natural (unwrapped) width of the title and body, widened
        // (up to MAX_WIDTH) to fit the buttons; beyond that their labels are elided to equal
        // shares.
        let title_w = ui.layout(view.heading, &title_style, f64::INFINITY, None).size.width;
        let body_w = ui.layout(view.body, &body_style, f64::INFINITY, None).size.width;
        let n = view.buttons.len();
        let mut labels: Vec<_> = view.buttons.iter().map(|l| ui.layout(l, &body_style, f64::INFINITY, None)).collect();
        let footer_gaps = 2.0 * FOOTER_MARGIN + BUTTON_GAP * n.saturating_sub(1) as f64;
        let footer_w = if n > 0 { labels.iter().map(|l| widgets::button_width(l)).sum::<f64>() + footer_gaps } else { 0.0 };
        let win_w = window_width(title_w.max(body_w)).max(footer_w.ceil()).min(MAX_WIDTH);
        if footer_w > MAX_WIDTH {
            let share = ((MAX_WIDTH - footer_gaps) / n as f64 - 2.0 * BUTTON_PAD_X).max(0.0);
            labels = ui.elide_labels(labels, view.buttons, &body_style, share);
        }
        let has_icon = view.has_icon();
        let x0 = MARGIN + if has_icon { ICON_SIZE + MARGIN } else { 0.0 };
        let col_w = win_w - MARGIN - x0;
        let footer_h = if n > 0 { FOOTER_H } else { 0.0 };

        // Content: the icon (top-aligned) left of a column of title / progress / body, 16 apart.
        let mut col = Column::new(x0, x0 + col_w, MARGIN);
        let mut first = true;
        let mut gap = |col: &mut Column| {
            if !std::mem::take(&mut first) {
                col.space(MARGIN);
            }
        };
        if has_icon {
            let r = Rect::from_origin_size(Point::new(MARGIN, MARGIN), Size::new(ICON_SIZE, ICON_SIZE));
            widgets::icon(ui, view, r);
            out.parts.icon = Some(r);
        }
        if !view.heading.is_empty() {
            let block = ui.layout(view.heading, &title_style, col_w, None);
            gap(&mut col);
            let r = col.row(block.size.height);
            ui.text_in(&block, r.origin(), col_w, tk.title_text);
            out.parts.heading = Some(r);
        }
        if let Some(progress) = view.progress {
            gap(&mut col);
            let r = col.row(PROGRESS_H);
            widgets::progress(ui, r, progress, &tk);
            out.parts.progress = Some(r);
        }
        if !view.body.is_empty() {
            // The body scrolls when the window would exceed the height limit.
            let block = ui.layout(view.body, &body_style, col_w, None);
            gap(&mut col);
            let rest = col.y + MARGIN + footer_h;
            let r = col.row(block.size.height.min((view.max_height - rest).max(MIN_VIEWPORT)));
            let viewport = Rect::new(x0, r.y0, win_w - SCROLLBAR_INSET, r.y1);
            let parts = &mut out.parts;
            ui.scroll_area(Id::new("ubuntu.scrollbar"),
                           &mut self.scroll,
                           viewport,
                           block.size.height,
                           &view.frame,
                           &widgets::SCROLL_BAR,
                           tk.scroll_thumb,
                           |ui, top| {
                               let at = Point::new(x0, top);
                               ui.text_in(&block, at, col_w, tk.body_text);
                               parts.body = Some(Rect::from_origin_size(at, Size::new(col_w, block.size.height)).intersect(viewport));
                           });
        }
        let row_h = (col.y - MARGIN).max(if has_icon { ICON_SIZE } else { 0.0 });
        let mut bottom = MARGIN + row_h + MARGIN;

        // Footer (only with buttons): buttons right-aligned in API order.
        if n > 0 {
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
            bottom += footer_h;
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

    /// Buttons wider than the text widen the window; past the maximum width their labels are
    /// elided, so every button stays inside the window.
    #[test]
    fn buttons_fit_the_window() {
        let long = |s: &str| format!("{s} with a fairly long label");
        let widened = vec!["Cancel".into(), "Don't Save".into(), "Save to Another Location".into()];
        for buttons in [vec![long("Cancel"), long("Retry"), long("OK")], widened] {
            let (out, _) = pass_with(&mut UbuntuTheme::new(), &content(&buttons), &mut state(), 0.0);
            let w = out.desired_size.width;
            assert!(w > MIN_WIDTH && w <= MAX_WIDTH, "{w}");
            assert_eq!(out.buttons.len(), 3);
            for b in &out.buttons {
                assert!(b.rect.x0 >= FOOTER_MARGIN - 1e-9 && b.rect.x1 <= w, "{:?} in {w}", b.rect);
            }
        }
    }

    /// A body taller than the height limit scrolls instead of growing the window.
    #[test]
    fn long_body_scrolls() {
        let buttons = vec!["OK".to_string()];
        let body = "A line of body text.\n".repeat(200);
        let v = DialogView { body: &body, max_height: 400.0, ..content(&buttons) };
        let mut theme = UbuntuTheme::new();
        let (out, shapes) = pass_with(&mut theme, &v, &mut state(), 0.0);
        assert!(out.desired_size.height <= 400.0, "{:?}", out.desired_size);
        let body_r = out.parts.body.expect("body");
        assert!(body_r.y1 <= out.buttons[0].rect.y0, "{body_r:?}");
        let thumb = UbuntuTokens::resolve(&Appearance::default()).scroll_thumb;
        assert!(shapes.iter().any(|s| matches!(s, crate::backends::draw::Shape::Rect { color, .. } if *color == thumb)), "scroll bar");
        // The wheel scrolls it while the pointer is over it.
        let mut st = state();
        st.pointer = Some(body_r.center());
        let wheel = DialogView { frame: FrameInfo { wheel_request: 40.0, ..v.frame }, ..v };
        pass_with(&mut theme, &wheel, &mut st, 0.0);
        assert_eq!(theme.scroll, 40.0);
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
