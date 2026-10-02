//! Fluent theme: the WinUI 3 ContentDialog "solid" look as a top-level window, built from the
//! theme's own widgets (`widgets.rs`: button, progress bar, severity icon, scroll bar).
//!
//! Layout (logical px, the standalone ContentDialog template):
//!
//! ```text
//! ┌──────────────────────────────────────┐  content area: LayerFillColorAlt, padding 24
//! │ Title (20 px SemiBold, ≤ 2 lines)    │
//! │ 12                                   │
//! │ [icon 32] 12 body (14 px, wraps)     │  a short body is centred on the icon
//! │ 12  ▬▬▬▬▬▬▬▬▬▬▬▬▬▬ progress (4 px)   │
//! ├──────────────────────────────────────┤  1 px separator (CardStrokeColorDefault)
//! │ [ Accent ] 8 [ Button ] 8 [ Button ] │  button bar: SolidBackgroundFillColorBase, padding 24
//! └──────────────────────────────────────┘
//! ```
//!
//! Width = widest wrapped line + 48 clamped to 320..548 (text wraps at 548 - 48 [- 44 with an
//! icon]); min height 184; above `min(756, view.max_height)` the icon/body row scrolls. A custom
//! icon (`XDialogIcon::Custom`) is a 48 px image at the top left, 16 px left of a column holding
//! the title, body and progress bar. Buttons are shown in REVERSED API order (the last API index,
//! the affirmative, first/left), in equal columns (one button: the right half); the default
//! button (last API index) is the accent button in message dialogs, and the accent follows focus.

mod tokens;
mod widgets;

use std::rc::Rc;

use self::tokens::{FluentTokens, BODY_SIZE, FONTS, TITLE_SIZE};
use self::widgets::{BUTTON_H, ICON_SIZE, PROGRESS_H};
use crate::backends::draw::{Color, Point, Rect, Size};
use crate::backends::gui::appearance::Appearance;
use crate::backends::gui::text::{TextBlock, TextStyle};
use crate::backends::gui::theme::*;
use crate::backends::gui::ui::{columns, Column};
use crate::model::XDialogIcon;

/// ContentDialogPadding (content area and button bar).
const PAD: f64 = 24.0;
/// Dialog width limits (ContentDialogMinWidth / MaxWidth) and minimum height.
const MIN_W: f64 = 320.0;
const MAX_W: f64 = 548.0;
const MIN_H: f64 = 184.0;
/// ContentDialogMaxHeight: above this (or the monitor limit) the body scrolls.
const MAX_H: f64 = 756.0;
/// ContentDialogTitleMargin bottom, icon column gap, progress StackPanel spacing.
const TITLE_GAP: f64 = 12.0;
const ICON_GAP: f64 = 12.0;
/// `XDialogIcon::Custom`: a bigger image at the top left, left of the title, body and progress
/// bar (which share one left edge), and its gap to them.
const CUSTOM_ICON_SIZE: f64 = 48.0;
const CUSTOM_ICON_GAP: f64 = 16.0;
const PROGRESS_GAP: f64 = 12.0;
/// The progress body StackPanel's MinWidth.
const PROGRESS_MIN_W: f64 = 300.0;
/// ContentDialogButtonSpacing.
const BUTTON_GAP: f64 = 8.0;
/// Button padding (11 + 11) + border (1 + 1): a column is never narrower than label + this.
const BUTTON_CHROME_W: f64 = 24.0;
/// Distance of the body viewport's right edge (where the overlay scroll bar sits) from the
/// window's right edge.
const SCROLLBAR_INSET: f64 = 4.0;
/// Smallest scrolling body viewport.
const MIN_VIEWPORT: f64 = 40.0;

/// Keyboard policy (WinUI): focus ring only after keyboard navigation (not on open: the default
/// button is marked by its accent fill), clamped arrows, Enter falls back to the default button,
/// Space activates on release, PageUp/PageDown/Home/End scroll the body.
pub(crate) const KEYBOARD: KeyboardPolicy = KeyboardPolicy { focus_visibility: FocusVisibility::KeyboardOnly,
                                                             arrows: ArrowNav::Clamp,
                                                             enter_falls_back_to_default: true,
                                                             enter_activates_default: false,
                                                             space: SpaceKey::ActivateOnRelease,
                                                             scroll_keys: true };

/// The Fluent theme.
pub(crate) struct FluentTheme {
    tokens: FluentTokens,
    /// Body scroll offset, logical px.
    scroll: f64,
}

impl FluentTheme {
    pub(crate) fn new() -> Self {
        FluentTheme { tokens: FluentTokens::new(&Appearance::default()), scroll: 0.0 }
    }
}

impl Theme for FluentTheme {
    fn set_appearance(&mut self, appearance: &Appearance) {
        self.tokens = FluentTokens::new(appearance);
    }

    fn keyboard_policy(&self) -> KeyboardPolicy {
        KEYBOARD
    }

    fn fonts(&self) -> &'static ThemeFonts {
        &FONTS
    }

    fn icon_size(&self) -> f64 {
        CUSTOM_ICON_SIZE
    }

    fn clear_color(&self) -> Color {
        self.tokens.bar_bg
    }

    fn ui(&mut self, view: &DialogView<'_>, ui: &mut Ui<'_>) -> DialogUiOutput {
        let tk = self.tokens.clone();
        let body_style = TextStyle::regular(BODY_SIZE);
        let title_style = TextStyle::bold(TITLE_SIZE);
        let mut out = DialogUiOutput::default();

        // ---- measure (wrap once at the maximum column, then shrink to the widest line) ----
        // A custom image leads the whole content column; a severity icon sits left of the body.
        let custom = view.custom_icon.filter(|_| *view.icon == XDialogIcon::Custom);
        let lead = if custom.is_some() { CUSTOM_ICON_SIZE + CUSTOM_ICON_GAP } else { 0.0 };
        let has_icon = view.has_icon() && custom.is_none();
        let icon_col = if has_icon { ICON_SIZE + ICON_GAP } else { 0.0 };
        let col_max = MAX_W - 2.0 * PAD - lead;
        let title = (!view.heading.is_empty()).then(|| ui.layout(view.heading, &title_style, col_max, Some(2)));
        let body = ui.layout(view.body, &body_style, col_max - icon_col, None);
        let n = view.buttons.len();
        let labels: Vec<Rc<TextBlock>> = view.buttons.iter().map(|b| ui.layout(b, &body_style, f64::INFINITY, Some(1))).collect();

        let body_w = if has_icon { ICON_SIZE + if body.is_empty() { 0.0 } else { ICON_GAP + body.size.width } } else { body.size.width };
        let mut need = title.as_ref().map_or(0.0, |t| t.size.width).max(body_w);
        if view.progress.is_some() {
            need = need.max(PROGRESS_MIN_W);
        }
        need += lead;
        // Equal button columns: the buttons fill the last `n` of `slots`.
        let slots = n.max(2);
        if n > 0 {
            let widest = labels.iter().map(|l| l.size.width).fold(0.0, f64::max) + BUTTON_CHROME_W;
            need = need.max(slots as f64 * widest + BUTTON_GAP * (slots - 1) as f64);
        }
        let win_w = (need + 2.0 * PAD).ceil().clamp(MIN_W, MAX_W);
        let col_w = win_w - 2.0 * PAD;
        // The content column (right of a custom icon).
        let (cx, ccol_w) = (PAD + lead, col_w - lead);
        // Labels wider than their column (window at MAX_W) are elided to it.
        let label_max = (col_w - BUTTON_GAP * (slots - 1) as f64) / slots as f64 - BUTTON_CHROME_W;
        let labels = ui.elide_labels(labels, view.buttons, &body_style, label_max);

        // ---- content area (painted first: it is the backdrop of everything above the bar) ----
        let has_row = has_icon || !body.is_empty();
        let row_h = if has_icon { body.size.height.max(ICON_SIZE) } else { body.size.height };
        let progress_gap = if has_row || title.is_some() { PROGRESS_GAP } else { 0.0 };
        let progress_h = if view.progress.is_some() { progress_gap + PROGRESS_H } else { 0.0 };
        // Button bar + separator (none without buttons: ContentDialog collapses CommandSpace).
        let bar_h = if n > 0 { 2.0 * PAD + BUTTON_H + 1.0 } else { 0.0 };
        let max_h = view.max_height.min(MAX_H);
        let mut col = Column::new(cx, cx + ccol_w, PAD);
        let title_h = title.as_ref().map_or(0.0, |t| t.size.height + if has_row || view.progress.is_some() { TITLE_GAP } else { 0.0 });
        let row_top = PAD + title_h;
        // Whatever the rest of the dialog leaves of the height limit.
        let viewport_h = if has_row { row_h.min((max_h - row_top - progress_h - PAD - bar_h).max(MIN_VIEWPORT)) } else { 0.0 };
        let mut inner_h = title_h + viewport_h + progress_h;
        if custom.is_some() {
            inner_h = inner_h.max(CUSTOM_ICON_SIZE);
        }
        // ContentDialog's minimum height; a button-less progress dialog hugs its content.
        let content_h = (inner_h + 2.0 * PAD).max(if n > 0 { MIN_H - bar_h } else { 0.0 });
        ui.fill_rect(Rect::new(0.0, 0.0, win_w, content_h), 0.0, tk.content_bg);

        if let Some(image) = custom {
            let r = Rect::from_origin_size(Point::new(PAD, PAD), Size::new(CUSTOM_ICON_SIZE, CUSTOM_ICON_SIZE));
            ui.image(image, r);
            out.parts.icon = Some(r);
        }
        if let Some(t) = &title {
            let r = col.row(t.size.height);
            ui.text_in(t, r.origin(), ccol_w, tk.text);
            out.parts.heading = Some(Rect::from_origin_size(r.origin(), t.size));
            col.space(title_h - t.size.height);
        }
        if has_row {
            // The viewport reaches into the right padding, so the overlay scroll bar sits next to
            // the text, as WinUI's ScrollViewer around the padded content.
            let viewport = Rect::new(cx, col.y, win_w - SCROLLBAR_INSET, col.y + viewport_h);
            let parts = &mut out.parts;
            ui.scroll_area(Id::new("fluent.scrollbar"),
                           &mut self.scroll,
                           viewport,
                           row_h,
                           &view.frame,
                           &widgets::SCROLL_BAR,
                           tk.scroll_thumb,
                           |ui, top| {
                               if has_icon {
                                   // A short body is centred on the icon; a taller one starts level with it.
                                   widgets::icon(ui, Point::new(cx, top), view.icon, &tk);
                                   let r = Rect::from_origin_size(Point::new(cx, top), Size::new(ICON_SIZE, ICON_SIZE));
                                   parts.icon = Some(r.intersect(viewport));
                               }
                               if !body.is_empty() {
                                   let pos = Point::new(cx + icon_col, top + (row_h - body.size.height) / 2.0);
                                   ui.text_in(&body, pos, ccol_w - icon_col, tk.text);
                                   let r = Rect::from_origin_size(pos, Size::new(ccol_w - icon_col, body.size.height));
                                   parts.body = Some(r.intersect(viewport));
                               }
                           });
            col.space(viewport_h);
        }
        if let Some(p) = view.progress {
            col.space(progress_gap);
            let r = col.row(PROGRESS_H);
            widgets::progress(ui, r, p, &tk);
            out.parts.progress = Some(r);
        }

        // ---- button bar ----
        let mut win_h = content_h;
        if n > 0 {
            ui.fill_rect(Rect::new(0.0, content_h, win_w, content_h + 1.0), 0.0, tk.separator);
            let row = Rect::new(PAD, content_h + 1.0 + PAD, PAD + col_w, content_h + 1.0 + PAD + BUTTON_H);
            // The default button (the last API index; core focuses it on open) is the accent
            // button unless another button has focus (accent follows focus; a press moved focus
            // before this pass).
            let default_button = n - 1;
            let focused = ui.focused_button();
            let accent =
                |i: usize| view.progress.is_none() && i == default_button && (focused.is_none() || focused == Some(default_button));
            // Equal columns (ContentDialog CommandSpace): the buttons fill the last `n` slots, so
            // one button sits in the right half.
            let cells = columns(row, slots, BUTTON_GAP);
            for k in 0..n {
                let index = n - 1 - k;
                let st = widgets::button(ui, cells[slots - n + k], index, &labels[index], accent(index), view, &tk);
                out.push_button(&st);
            }
            win_h = row.y1 + PAD;
        }
        out.desired_size = Size::new(win_w, win_h);
        out
    }
}

#[cfg(all(test, draw_soft))]
mod tests {
    use super::*;
    use crate::backends::gui::theme::test_support::{pass, pass_with, state, view};

    /// The default (last API) button is on the left; the size comes from the laid-out rects.
    #[test]
    fn buttons_are_reversed_with_the_default_first() {
        let mut theme = FluentTheme::new();
        let buttons = vec!["No".to_string(), "Yes".to_string()];
        let view = DialogView { heading: "Save changes?", body: "Body.", ..view(&buttons) };
        let (out, _) = pass(&mut theme, &view);
        assert_eq!(out.buttons.iter().map(|b| b.index).collect::<Vec<_>>(), vec![1, 0]);
        assert!(out.buttons[0].rect.x0 < out.buttons[1].rect.x0);
        assert!(out.desired_size.height >= MIN_H);
        assert_eq!(out.buttons[0].rect.y1, out.desired_size.height - PAD);
        assert_eq!(out.buttons[0].rect.x0, PAD);
        assert_eq!(out.buttons[1].rect.x1, out.desired_size.width - PAD);
        assert!(out.parts.heading.is_some() && out.parts.body.is_some() && out.parts.icon.is_none());
    }

    /// The wheel scrolls the body only while the pointer is over it; keys scroll it anywhere.
    #[test]
    fn wheel_scrolls_the_body_only_under_the_pointer() {
        let mut theme = FluentTheme::new();
        let buttons = vec!["OK".to_string()];
        let body = "A long line of body text. ".repeat(80);
        let base = DialogView { body: &body, max_height: 300.0, ..view(&buttons) };
        let mut st = state();
        let (out, _) = pass_with(&mut theme, &base, &mut st, 0.0);
        let body_r = out.parts.body.expect("body");
        let wheel = DialogView { frame: FrameInfo { wheel_request: 40.0, ..Default::default() }, ..base };
        // Outside the window, then over the button bar: no scroll.
        pass_with(&mut theme, &wheel, &mut st, 0.0);
        st.pointer = Some(out.buttons[0].rect.center());
        pass_with(&mut theme, &wheel, &mut st, 0.0);
        assert_eq!(theme.scroll, 0.0);
        // Over the body.
        st.pointer = Some(body_r.center());
        pass_with(&mut theme, &wheel, &mut st, 0.0);
        assert_eq!(theme.scroll, 40.0);
        // Keys scroll wherever the pointer is.
        st.pointer = None;
        let keys = DialogView { frame: FrameInfo { scroll_request: 10.0, ..Default::default() }, ..base };
        pass_with(&mut theme, &keys, &mut st, 0.0);
        assert_eq!(theme.scroll, 50.0);
    }
}
