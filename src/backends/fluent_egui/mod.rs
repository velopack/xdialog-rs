//! Fluent theme: the WinUI 3 ContentDialog "solid" look as a top-level window, built with egui
//! containers from the theme's own widgets (`widgets.rs`: `FluentButton`, `FluentProgress`,
//! `FluentIcon`).
//!
//! Layout (logical px, the standalone ContentDialog template):
//!
//! ```text
//! ┌──────────────────────────────────────┐  content area: LayerFillColorAlt, padding 24
//! │ Title (20 px SemiBold, ≤ 2 lines)    │
//! │ 12                                   │
//! │ [icon 32] 12 body (14 px, wraps)     │  body vertically centred on the icon row
//! │ 12  ▬▬▬▬▬▬▬▬▬▬▬▬▬▬ progress (3 px)   │
//! ├──────────────────────────────────────┤  1 px separator (CardStrokeColorDefault)
//! │ [ Accent ] 8 [ Button ] 8 [ Button ] │  button bar: SolidBackgroundFillColorBase, padding 24
//! └──────────────────────────────────────┘
//! ```
//!
//! Width = widest wrapped line + 48 clamped to 320..548 (text wraps at 548 - 48 [- 44 with an
//! icon]); min height 184; above `min(756, limits.max_height)` the icon/body row scrolls
//! (`egui::ScrollArea`). Buttons are shown in REVERSED API order (the last API index, the
//! affirmative, first/left), in equal columns (one button: the right half); the default button
//! (last API index) is the accent button in message dialogs, and the accent follows focus.

mod fonts;
mod tokens;
mod widgets;

use egui::{Color32, Frame, Margin, Sense, Ui, Vec2};

pub(crate) use self::tokens::FluentTokens;
use self::fonts::{FaceSet, BODY_SIZE, LINE_RATIO, TITLE_SIZE};
use self::widgets::{FluentButton, FluentIcon, FluentProgress, BUTTON_H, ICON_SIZE, PROGRESS_H};
use crate::backends::egui_core::appearance::Appearance;
use crate::backends::egui_core::fonts::FontRegistry;
use crate::backends::egui_core::text::{TextBlock, TextBlockWidget, TextCtx, TextStyle};
use crate::backends::egui_core::theme::*;
use crate::model::XDialogIcon;

/// ContentDialogPadding (content area and button bar).
const PAD: f32 = 24.0;
/// Dialog width limits (ContentDialogMinWidth / MaxWidth) and minimum height.
const MIN_W: f32 = 320.0;
const MAX_W: f32 = 548.0;
const MIN_H: f32 = 184.0;
/// ContentDialogMaxHeight: above this (or the monitor limit) the body scrolls.
const MAX_H: f32 = 756.0;
/// ContentDialogTitleMargin bottom, icon column gap, progress StackPanel spacing.
const TITLE_GAP: f32 = 12.0;
const ICON_GAP: f32 = 12.0;
const PROGRESS_GAP: f32 = 12.0;
/// The progress body StackPanel's MinWidth.
const PROGRESS_MIN_W: f32 = 300.0;
/// ContentDialogButtonSpacing.
const BUTTON_GAP: f32 = 8.0;
/// Button padding (11 + 11) + border (1 + 1): a column is never narrower than label + this.
const BUTTON_CHROME_W: f32 = 24.0;
/// Distance of the body scroll bar's viewport edge from the window's right edge.
const SCROLLBAR_INSET: f32 = 4.0;
/// Smallest scrolling body viewport.
const MIN_VIEWPORT: f32 = 40.0;

/// The Fluent theme.
pub(crate) struct FluentTheme {
    faces: FaceSet,
}

impl FluentTheme {
    pub(crate) fn new() -> Self {
        FluentTheme { faces: FaceSet::get(FontRegistry::global()) }
    }
}

impl Theme for FluentTheme {
    type Tokens = FluentTokens;

    fn keyboard_policy(&self) -> KeyboardPolicy {
        KeyboardPolicy { focus_visibility: FocusVisibility::KeyboardOnly,
                         arrows: ArrowNav::Clamp,
                         enter_falls_back_to_default: true,
                         space: SpaceKey::ActivateOnRelease,
                         scroll_keys: true }
    }

    fn tokens(&self, appearance: &Appearance) -> FluentTokens {
        FluentTokens::new(appearance)
    }

    fn fonts(&self) -> ThemeFonts {
        self.faces.fonts()
    }

    fn configure_style(&self, tk: &FluentTokens, style: &mut egui::Style) {
        style.spacing.item_spacing = Vec2::ZERO;
        style.spacing.interact_size = Vec2::ZERO;
        // WinUI ScrollBar: a thin 2 px thumb (ControlStrongFillColorDefault) that widens on hover.
        let mut scroll = egui::style::ScrollStyle::floating();
        scroll.bar_width = 6.0;
        scroll.floating_width = 2.0;
        scroll.bar_outer_margin = 2.0;
        scroll.bar_inner_margin = 0.0;
        scroll.dormant_background_opacity = 0.0;
        scroll.active_background_opacity = 0.0;
        scroll.interact_background_opacity = 0.0;
        scroll.dormant_handle_opacity = 1.0;
        scroll.active_handle_opacity = 1.0;
        scroll.interact_handle_opacity = 1.0;
        scroll.foreground_color = false;
        style.spacing.scroll = scroll;
        for w in [&mut style.visuals.widgets.inactive, &mut style.visuals.widgets.hovered, &mut style.visuals.widgets.active] {
            w.bg_fill = tk.scroll_thumb;
            w.weak_bg_fill = tk.scroll_thumb;
            w.fg_stroke.color = tk.scroll_thumb;
            w.corner_radius = egui::CornerRadius::same(3);
        }
        style.visuals.extreme_bg_color = Color32::TRANSPARENT;
        style.visuals.panel_fill = tk.bar_bg;
        style.visuals.window_fill = tk.bar_bg;
    }

    fn ui(&self, tk: &FluentTokens, view: &DialogView<'_>, ui: &mut Ui) -> DialogUiOutput {
        let ctx = ui.ctx().clone();
        let text = TextCtx::new(&ctx);
        let body_style = TextStyle::regular(BODY_SIZE, BODY_SIZE * LINE_RATIO);
        let title_style = TextStyle::bold(TITLE_SIZE, TITLE_SIZE * LINE_RATIO);

        // ---- measure (wrap once at the maximum column, then shrink to the widest line) ----
        let has_icon = *view.icon != XDialogIcon::None;
        let icon_col = if has_icon { ICON_SIZE + ICON_GAP } else { 0.0 };
        let col_max = MAX_W - 2.0 * PAD;
        let title = (!view.heading.is_empty()).then(|| text.layout(view.heading, &title_style, col_max, Some(2)));
        let body = text.layout(view.body, &body_style, col_max - icon_col, None);
        let n = view.buttons.len();
        let labels: Vec<TextBlock> = view.buttons.iter().map(|b| text.layout(b, &body_style, f32::INFINITY, Some(1))).collect();

        let body_w = if has_icon { ICON_SIZE + if body.is_empty() { 0.0 } else { ICON_GAP + body.size.x } } else { body.size.x };
        let mut need = title.as_ref().map_or(0.0, |t| t.size.x).max(body_w);
        if view.progress.is_some() {
            need = need.max(PROGRESS_MIN_W);
        }
        // Equal button columns: the buttons fill the last `n` of `slots`.
        let slots = n.max(2) as f32;
        if n > 0 {
            let widest = labels.iter().map(|l| l.size.x).fold(0.0, f32::max) + BUTTON_CHROME_W;
            need = need.max(slots * widest + BUTTON_GAP * (slots - 1.0));
        }
        let win_w = (need + 2.0 * PAD).ceil().clamp(MIN_W, MAX_W);
        let col_w = win_w - 2.0 * PAD;
        // Labels wider than their column (window at MAX_W) are elided to it.
        let label_max = (col_w - BUTTON_GAP * (slots - 1.0)) / slots - BUTTON_CHROME_W;
        let labels: Vec<TextBlock> = labels.into_iter()
                                           .zip(view.buttons)
                                           .map(|(l, b)| if l.size.x > label_max { text.layout(b, &body_style, label_max, Some(1)) } else { l })
                                           .collect();

        // ---- content area ----
        let has_row = has_icon || !body.is_empty();
        let row_h = if has_icon { body.size.y.max(ICON_SIZE) } else { body.size.y };
        let progress_gap = if has_row || title.is_some() { PROGRESS_GAP } else { 0.0 };
        let progress_h = if view.progress.is_some() { progress_gap + PROGRESS_H } else { 0.0 };
        // Button bar + separator (none without buttons: ContentDialog collapses CommandSpace).
        let bar_h = if n > 0 { 2.0 * PAD + BUTTON_H + 1.0 } else { 0.0 };
        let max_h = view.limits.max_height.min(MAX_H);
        let content = Frame::new().fill(tk.content_bg).inner_margin(PAD).show(ui, |ui| {
            ui.set_width(col_w);
            // ContentDialog's minimum height; a button-less progress dialog hugs its content.
            if n > 0 {
                ui.set_min_height(MIN_H - bar_h - 2.0 * PAD);
            }
            if let Some(t) = &title {
                ui.add(TextBlockWidget { block: t, color: tk.text, width: Some(col_w) });
                if has_row || view.progress.is_some() {
                    ui.add_space(TITLE_GAP);
                }
            }
            if has_row {
                // Whatever the rest of the dialog leaves of the height limit.
                let viewport = (max_h - ui.cursor().top() - progress_h - PAD - bar_h).max(MIN_VIEWPORT);
                let mut area = egui::ScrollArea::vertical().id_salt("fluent.body").auto_shrink([false, true]).max_height(viewport);
                if view.frame.scroll_request != 0.0 {
                    // Keyboard scroll: set the offset BEFORE `show` so this pass lays the content
                    // out at it (`scroll_with_delta` inside the area only moves the content on the
                    // next pass). Same id as `ScrollArea::show`.
                    let id = ui.make_persistent_id("fluent.body");
                    let offset = egui::scroll_area::State::load(ui.ctx(), id).map_or(0.0, |s| s.offset.y);
                    area = area.vertical_scroll_offset((offset + view.frame.scroll_request).clamp(0.0, (row_h - viewport).max(0.0)));
                }
                // The viewport reaches into the right padding, so the (overlay) scroll bar sits
                // next to the text, as WinUI's ScrollViewer around the padded content.
                let into_padding = Margin { right: -(PAD - SCROLLBAR_INSET) as i8, ..Margin::ZERO };
                Frame::new().outer_margin(into_padding).show(ui, |ui| {
                    area.show(ui, |ui| {
                            ui.set_max_width(col_w);
                            ui.horizontal(|ui| {
                                  if has_icon {
                                      ui.add(FluentIcon { icon: view.icon, tk });
                                      if !body.is_empty() {
                                          ui.add_space(ICON_GAP);
                                      }
                                  }
                                  if !body.is_empty() {
                                      ui.add(TextBlockWidget { block: &body, color: tk.text, width: Some(col_w - icon_col) });
                                  }
                              });
                        });
                });
            }
            if let Some(p) = view.progress {
                ui.add_space(progress_gap);
                ui.add(FluentProgress { progress: p, tk });
            }
        });
        let mut win_h = content.response.rect.bottom();

        // ---- button bar ----
        let default_button = n.checked_sub(1);
        let mut out = DialogUiOutput { default_button, ..Default::default() };
        if n > 0 {
            let (sep, _) = ui.allocate_exact_size(Vec2::new(win_w, 1.0), Sense::hover());
            ui.painter().rect_filled(sep, 0.0, tk.separator);
            // Accent follows focus: the default button is accent unless another button has focus.
            // A primary press on a button moves focus to it during this pass (in
            // `ButtonInteraction`), after the buttons left of it were painted: take it into
            // account up front so the accent moves in the same frame as the press look. egui
            // already hit-tested the press at the start of the pass (`read_response` exposes it,
            // the same `is_pointer_button_down_on` that `ButtonInteraction` reads).
            let focused = ctx.memory(|m| m.focused());
            let pressed_button = ctx.input(|i| i.pointer.primary_pressed())
                                    .then(|| {
                                        (0..n).find(|&i| ctx.read_response(button_id(i)).is_some_and(|r| r.is_pointer_button_down_on()))
                                    })
                                    .flatten();
            let focused_button = pressed_button.or_else(|| (0..n).find(|&i| focused == Some(button_id(i))));
            let accent = |i: usize| {
                view.kind == DialogKind::Message && Some(i) == default_button && (focused_button.is_none() || focused_button == default_button)
            };
            let bar = Frame::new().inner_margin(PAD).show(ui, |ui| {
                ui.set_width(col_w);
                ui.spacing_mut().item_spacing.x = BUTTON_GAP;
                // Equal columns (ContentDialog CommandSpace): the buttons fill the last `n` slots,
                // so one button sits in the right half.
                let slots = n.max(2);
                ui.columns(slots, |cols| {
                      for k in 0..n {
                          let index = n - 1 - k;
                          let st = FluentButton { index, label: &labels[index], accent: accent(index), view, tk }.show(&mut cols[slots - n + k]);
                          out.push_button(&st);
                          out.arrow_order.push(index);
                      }
                  });
            });
            win_h = bar.response.rect.bottom();
        }
        out.desired_size = Vec2::new(win_w, win_h);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::egui_core::theme::test_support::{theme_ctx, view};
    use egui::{Pos2, Rect};

    /// The default (last API) button is on the left and gets initial focus.
    #[test]
    fn buttons_are_reversed_with_the_default_first() {
        let theme = FluentTheme::new();
        let tk = theme.tokens(&Appearance::default());
        let ctx = theme_ctx(&theme, &tk);
        let buttons = vec!["No".to_string(), "Yes".to_string()];
        let view = DialogView { heading: "Save changes?", body: "Body.", ..view(&buttons) };
        let mut out = DialogUiOutput::default();
        let raw = egui::RawInput { time: Some(0.0), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 400.0))), ..Default::default() };
        ctx.run_ui(raw, |ui| out = theme.ui(&tk, &view, ui)).textures_delta.clear();
        assert_eq!(out.default_button, Some(1));
        assert_eq!(out.arrow_order, vec![1, 0]);
        assert!(out.buttons[0].rect.left() < out.buttons[1].rect.left());
        // The size comes from the laid-out rects: 24 px padding below the buttons.
        assert!(out.desired_size.y >= MIN_H);
        assert_eq!(out.buttons[0].rect.bottom(), out.desired_size.y - PAD);
        assert_eq!(out.buttons[0].rect.left(), PAD);
        assert_eq!(out.buttons[1].rect.right(), out.desired_size.x - PAD);
    }
}
