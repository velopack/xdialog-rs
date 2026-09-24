//! Fluent theme: the WinUI 3 ContentDialog "solid" look as a top-level window, measured from
//! WinUI 3 captures and the WinUI theme resources. Widget-based: the dialog is built with egui layout from the theme's own widgets (`widgets.rs`: `FluentButton`,
//! `FluentProgress`, `FluentIcon`); severity icons are procedural.
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
//! affirmative, first/left), in equal star columns (one button: the right half); the default
//! button (last API index) is the accent button in message dialogs, and the accent follows focus.

mod fonts;
mod tokens;
mod widgets;

use egui::{Align, Color32, FontFamily, Layout, Pos2, Rect, Shape, Ui, UiBuilder, Vec2};

pub(crate) use self::tokens::FluentTokens;
use self::fonts::{FaceSet, BODY_SIZE, LINE_RATIO, TITLE_SIZE};
use self::widgets::{FluentButton, FluentIcon, FluentProgress, FluentText, BUTTON_H, ICON_SIZE, PROGRESS_H};
use crate::backends::egui_core::fonts::{FaceRef, FontRegistry};
use crate::backends::egui_core::paint_util::snap_len;
use crate::backends::egui_core::text::{TextBlock, TextCtx, TextStyle};
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

    fn body_style(&self) -> TextStyle {
        TextStyle { round_line_tops: true, ..TextStyle::new(BODY_SIZE, FontFamily::Proportional, BODY_SIZE * LINE_RATIO) }
    }

    fn title_style(&self) -> TextStyle {
        TextStyle { round_line_tops: true, ..TextStyle::new(TITLE_SIZE, fonts::title_family(), TITLE_SIZE * LINE_RATIO) }
    }
}

impl Theme for FluentTheme {
    type Tokens = FluentTokens;

    fn id(&self) -> &'static str {
        "fluent"
    }

    fn keyboard_policy(&self) -> KeyboardPolicy {
        KeyboardPolicy { focus_on_open: true,
                         focus_visibility: FocusVisibility::KeyboardOnly { on_open: true },
                         tab: true,
                         arrows: ArrowNav::Clamp,
                         nav_repeat: true,
                         home_end: false,
                         enter: true,
                         enter_falls_back_to_default: true,
                         space: SpaceKey::ActivateOnRelease,
                         activate_flash: None,
                         escape_closes: true,
                         scroll_keys: true }
    }

    fn tokens(&self, env: &ThemeEnv) -> FluentTokens {
        FluentTokens::new(env)
    }

    fn window_style(&self, tk: &FluentTokens) -> WindowStyle {
        WindowStyle { clear: tk.bar_bg, dark_titlebar: tk.dark }
    }

    fn install_fonts(&self, defs: &mut egui::FontDefinitions, _reg: &FontRegistry) {
        self.faces.install(defs);
    }

    fn text_families(&self) -> Vec<FontFamily> {
        vec![FontFamily::Proportional, FontFamily::Monospace, fonts::title_family()]
    }

    /// The title is SemiBold; DirectWrite's fallback picks the bold face of a 400/700 family for it.
    fn bold_families(&self) -> Vec<FontFamily> {
        vec![fonts::title_family()]
    }

    fn primary_faces(&self, _reg: &FontRegistry) -> Vec<FaceRef> {
        self.faces.faces()
    }

    fn configure_style(&self, tk: &FluentTokens, style: &mut egui::Style) {
        style.spacing.item_spacing = Vec2::ZERO;
        // One atlas gamma for both themes; light-on-dark text gets its extra weight in `paint_text`.
        style.visuals.text_options.color_transfer_function = egui::epaint::FontColorTransferFunction::Gamma(widgets::TEXT_GAMMA);
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
        style.visuals.panel_fill = tk.content_bg;
        style.visuals.window_fill = tk.content_bg;
    }

    fn ui(&self, tk: &FluentTokens, view: &DialogView<'_>, ui: &mut Ui) -> DialogUiOutput {
        let ctx = ui.ctx().clone();
        let text = TextCtx::new(&ctx);
        let ppp = view.frame.ppp;
        let px = |v: f32| snap_len(v, ppp);
        let px_ceil = |v: f32| (v * ppp - 1e-3).ceil() / ppp;
        let body_style = self.body_style();
        let title_style = self.title_style();

        // ---- measure (wrap once at the maximum column, then shrink to the widest line) ----
        let has_icon = *view.icon != XDialogIcon::None;
        let icon_col = if has_icon { ICON_SIZE + ICON_GAP } else { 0.0 };
        let col_max = MAX_W - 2.0 * PAD;
        let title = (!view.heading.is_empty()).then(|| dwrite_lines(text.layout(view.heading, &title_style, col_max, Some(2)), &title_style, TITLE_DY, false));
        let body = dwrite_lines(text.layout(view.body, &body_style, col_max - icon_col, None), &body_style, BODY_DY, true);
        let n = view.buttons.len();
        let labels: Vec<TextBlock> = view.buttons.iter().map(|b| dwrite_lines(text.layout(b, &body_style, f32::INFINITY, Some(1)), &body_style, BODY_DY, true)).collect();

        let body_w = if has_icon { ICON_SIZE + if body.is_empty() { 0.0 } else { ICON_GAP + body.size.x } } else { body.size.x };
        let mut need = title.as_ref().map_or(0.0, |t| t.size.x).max(body_w);
        if view.progress.is_some() {
            need = need.max(PROGRESS_MIN_W);
        }
        if n > 0 {
            let slots = n.max(2) as f32;
            let widest = labels.iter().map(|l| l.size.x).fold(0.0, f32::max) + BUTTON_CHROME_W;
            need = need.max(slots * widest + BUTTON_GAP * (slots - 1.0));
        }
        // Whole logical px first (text widths differ by a fraction of a pixel between scales), so
        // every scale lays out the same logical size, then whole physical px.
        let win_w = px_ceil((need + 2.0 * PAD - 1e-3).ceil().clamp(MIN_W, MAX_W));
        let col_w = win_w - 2.0 * PAD;

        // ---- vertical metrics ----
        let has_title = title.is_some();
        let has_row = has_icon || !body.is_empty();
        let row_h = if has_icon { body.size.y.max(ICON_SIZE) } else { body.size.y };
        let title_h = title.as_ref().map_or(0.0, |t| t.size.y);
        let title_gap = if has_title && (has_row || view.progress.is_some()) { TITLE_GAP } else { 0.0 };
        let progress_gap = if has_row || has_title { PROGRESS_GAP } else { 0.0 };
        let progress_h = if view.progress.is_some() { progress_gap + PROGRESS_H } else { 0.0 };
        let bar_h = if n > 0 { 2.0 * PAD + BUTTON_H } else { 0.0 };
        let natural_top = PAD + title_h + title_gap + row_h + progress_h + PAD + 1.0;
        let max_h = view.limits.max_height.min(MAX_H);
        let overflow = (natural_top + bar_h - max_h).max(0.0);
        let viewport_h = if overflow > 0.0 { px((row_h - overflow).max(MIN_VIEWPORT.min(row_h))) } else { row_h };
        let top_h = (natural_top - (row_h - viewport_h)).max(MIN_H - bar_h);
        let win_h = top_h + bar_h;

        // ---- content area ----
        // Background band + separator painted behind the content (slots reserved first).
        let band = ui.painter().add(Shape::Noop);
        let content = Rect::from_min_size(Pos2::new(PAD, PAD), Vec2::new(col_w, top_h - 1.0 - 2.0 * PAD));
        ui.scope_builder(UiBuilder::new().max_rect(content).layout(Layout::top_down(Align::Min)), |ui| {
              if let Some(t) = &title {
                  ui.add(FluentText { block: t, color: tk.text, bg: tk.content_bg });
                  ui.add_space(title_gap);
              }
              if has_row {
                  let row = |ui: &mut Ui| {
                      ui.horizontal_top(|ui| {
                            if has_icon {
                                ui.add(FluentIcon { icon: view.icon, tk });
                                if !body.is_empty() {
                                    ui.add_space(ICON_GAP);
                                }
                            }
                            if !body.is_empty() {
                                ui.vertical(|ui| {
                                      // VerticalAlignment=Center against the icon, whole pixels.
                                      ui.add_space(px((row_h - body.size.y) / 2.0));
                                      ui.add(FluentText { block: &body, color: tk.text, bg: tk.content_bg });
                                  });
                            }
                        });
                  };
                  if viewport_h < row_h {
                      // The viewport spans the right padding too, so the (overlay) scroll bar sits
                      // next to the text, as WinUI's ScrollViewer around the padded content.
                      let top = ui.cursor().top();
                      let viewport = Rect::from_min_max(Pos2::new(PAD, top), Pos2::new(win_w - SCROLLBAR_INSET, top + viewport_h));
                      ui.scope_builder(UiBuilder::new().max_rect(viewport).layout(Layout::top_down(Align::Min)), |ui| {
                            let mut area = egui::ScrollArea::vertical().id_salt("fluent.body").auto_shrink([false, false]).max_height(viewport_h);
                            if view.frame.scroll_request != 0.0 {
                                // Keyboard scroll: set the offset BEFORE `show` so this pass lays the
                                // content out at it (`scroll_with_delta` inside the area only moves
                                // the content on the next pass, a frame the offscreen renderer and
                                // an idle window never paint). Same id as `ScrollArea::show`.
                                let id = ui.make_persistent_id("fluent.body");
                                let offset = egui::scroll_area::State::load(ui.ctx(), id).map_or(0.0, |s| s.offset.y);
                                area = area.vertical_scroll_offset((offset + view.frame.scroll_request).clamp(0.0, row_h - viewport_h));
                            }
                            area.show(ui, |ui| {
                                    ui.set_max_width(col_w);
                                    row(ui);
                                });
                        });
                  } else {
                      row(ui);
                  }
              }
              if let Some(p) = view.progress {
                  ui.add_space(progress_gap);
                  ui.add(FluentProgress { progress: p, width: col_w, tk });
              }
          });
        ui.painter().set(band,
                         if n == 0 {
                             // No button bar: no separator (ContentDialog collapses CommandSpace).
                             Shape::rect_filled(Rect::from_min_size(Pos2::ZERO, Vec2::new(win_w, top_h)), 0.0, tk.content_bg)
                         } else {
                             Shape::Vec(vec![Shape::rect_filled(Rect::from_min_size(Pos2::ZERO, Vec2::new(win_w, top_h - 1.0)), 0.0, tk.content_bg),
                                             Shape::rect_filled(Rect::from_min_size(Pos2::new(0.0, top_h - 1.0), Vec2::new(win_w, 1.0)), 0.0, tk.separator)])
                         });

        // ---- button bar ----
        let default_button = n.checked_sub(1);
        // Initial focus / Enter fallback: the default button, or (when it is disabled) the first
        // enabled button on screen, as WinUI focuses the first focusable command button.
        let focus_target = default_button.filter(|&d| !view.is_disabled(d)).or_else(|| (0..n).rev().find(|&i| !view.is_disabled(i)));
        let mut out = DialogUiOutput { arrow_axis: ArrowAxis::Horizontal, default_button: focus_target, ..Default::default() };
        if n > 0 {
            // Star columns (ContentDialog CommandSpace): one button sits in the right half.
            let slots = n.max(2);
            let first_slot = slots - n;
            let widths = star_widths(col_w - BUTTON_GAP * (slots - 1) as f32, slots, ppp);
            let edge = |k: usize| PAD + widths[..k].iter().sum::<f32>() + k as f32 * BUTTON_GAP;
            let col = |k: usize| widths[k];
            // Accent follows focus: the default button is accent unless another button has focus.
            // A primary press on a button moves focus to it during this pass (in
            // `ButtonInteraction`), after the buttons left of it were painted: take it into
            // account up front so the accent moves in the same frame as the press look. egui
            // already hit-tested the press at the start of the pass (`read_response` exposes it,
            // the same `is_pointer_button_down_on` that `ButtonInteraction` reads).
            let focused = ctx.memory(|m| m.focused());
            let pressed_button = ctx.input(|i| i.pointer.primary_pressed())
                                    .then(|| {
                                        (0..n).find(|&i| {
                                                  !view.is_disabled(i) && ctx.read_response(button_id(i)).is_some_and(|r| r.is_pointer_button_down_on())
                                              })
                                    })
                                    .flatten();
            let focused_button = pressed_button.or_else(|| (0..n).find(|&i| focused == Some(button_id(i))));
            // A disabled default button keeps its (disabled) accent look.
            let accent = |i: usize| {
                view.kind == DialogKind::Message
                && Some(i) == default_button
                && (focused_button.is_none() || focused_button == default_button || view.is_disabled(i))
            };
            let row = Rect::from_min_size(Pos2::new(PAD, top_h + PAD), Vec2::new(col_w, BUTTON_H));
            ui.scope_builder(UiBuilder::new().max_rect(row).layout(Layout::left_to_right(Align::Min)), |ui| {
                  let mut x = PAD;
                  for k in 0..n {
                      let slot = first_slot + k;
                      let (x0, x1) = (edge(slot), edge(slot) + col(slot));
                      ui.add_space(x0 - x);
                      let index = n - 1 - k;
                      let r = ui.add(FluentButton { index, label: &labels[index], width: x1 - x0, accent: accent(index), view, tk });
                      if r.clicked() && r.contains_pointer() && !view.is_disabled(index) {
                          out.activated = Some(index);
                      }
                      out.buttons.push(ButtonInfo { index, rect: r.rect });
                      out.arrow_order.push(index);
                      x = x1;
                  }
              });
        }
        out.desired_size = Vec2::new(win_w, win_h);
        out
    }
}

/// Split `total` logical px into `n` equal star columns in whole physical pixels, the remainder
/// going to the first columns (XAML layout rounding: 297 -> 94, 94, 93 (+ gaps); 379 -> 190, 189).
fn star_widths(total: f32, n: usize, ppp: f32) -> Vec<f32> {
    let phys = (total * ppp).round().max(0.0) as u32;
    let n32 = n.max(1) as u32;
    let (base, rem) = (phys / n32, phys % n32);
    (0..n32).map(|k| (base + u32::from(k < rem)) as f32 / ppp).collect()
}

/// Fraction DirectWrite effectively adds before rounding a line's position to whole pixels
/// (fitted to the WinUI captures: ink tops of 1-, 2- and 6-line bodies and 1-line titles).
const LINE_ROUND_BIAS: f32 = 0.19;
/// Vertical glyph offset vs epaint's placement (fitted to the reference ink tops).
const BODY_DY: f32 = -1.0;
const TITLE_DY: f32 = 0.0;

/// Re-place the lines of `block` like XAML/DirectWrite: the TextBlock's desired height is rounded
/// up to whole pixels (`ceil(lines * pitch)`); dialog content (not the title, which sits in an
/// Auto row) is centred in that box; line `k` is placed at `round(offset + k * pitch + bias)`. (A 2-line body thus steps 18 px from line 0 to 1,
/// a 6-line body 19, 19, 18, 19, 18, exactly as in the captures.)
fn dwrite_lines(mut block: TextBlock, style: &TextStyle, dy: f32, centred: bool) -> TextBlock {
    if block.lines.is_empty() {
        return block;
    }
    let pitch = style.line_pitch;
    let exact = block.lines.len() as f32 * pitch;
    let height = (exact - 1e-3).ceil();
    let centre = if centred { (height - exact) / 2.0 } else { 0.0 };
    for (k, line) in block.lines.iter_mut().enumerate() {
        let half_leading = line.offset.y - line.box_top;
        line.box_top = (centre + k as f32 * pitch + LINE_ROUND_BIAS).round();
        line.offset.y = line.box_top + half_leading + dy;
    }
    block.size.y = height;
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_pitch_matches_directwrite() {
        assert!((BODY_SIZE * LINE_RATIO - 18.62).abs() < 0.01);
        assert!((TITLE_SIZE * LINE_RATIO - 26.6).abs() < 0.01);
    }

    /// One pass of the theme over a yes/no dialog with the given disabled flags.
    fn yesno_output(disabled: &[bool]) -> DialogUiOutput {
        use crate::backends::egui_core::appearance::Appearance;
        let theme = FluentTheme::new();
        let env = ThemeEnv { appearance: Appearance::default(), platform: Platform::current() };
        let tk = theme.tokens(&env);
        let ctx = egui::Context::default();
        let mut defs = egui::FontDefinitions::default();
        theme.install_fonts(&mut defs, FontRegistry::global());
        ctx.set_fonts(defs);
        let (icon, buttons) = (XDialogIcon::None, vec!["No".to_string(), "Yes".to_string()]);
        let view = DialogView { kind: DialogKind::Message,
                                title: "",
                                heading: "Save changes?",
                                body: "Body.",
                                icon: &icon,
                                buttons: &buttons,
                                disabled,
                                progress: None,
                                env: &env,
                                limits: SizeLimits { max_height: 800.0, max_width: 4096.0 },
                                frame: FrameInfo { time: 0.0,
                                                   ppp: 1.0,
                                                   sizing: false,
                                                   window_focused: true,
                                                   focus_visible: false,
                                                   key_pressed: None,
                                                   scroll_request: 0.0 } };
        let mut out = DialogUiOutput::default();
        let raw = egui::RawInput { time: Some(0.0), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 400.0))), ..Default::default() };
        ctx.run_ui(raw, |ui| out = theme.ui(&tk, &view, ui)).textures_delta.clear();
        out
    }

    /// Initial focus goes to the default button, or to the first enabled button on screen when
    /// the default one is disabled (WinUI focuses the first focusable command button).
    #[test]
    fn focus_target_skips_a_disabled_default() {
        assert_eq!(yesno_output(&[false, false]).default_button, Some(1));
        assert_eq!(yesno_output(&[false, true]).default_button, Some(0));
        assert_eq!(yesno_output(&[true, true]).default_button, None);
    }

    /// XAML star columns: the remainder pixels go to the first columns (captures: 345 px wide
    /// 3-button dialog -> 94, 94, 93; 435 px 2-button -> 190, 189; HiDPI in physical pixels).
    #[test]
    fn star_columns_round_like_xaml() {
        assert_eq!(star_widths(281.0, 3, 1.0), vec![94.0, 94.0, 93.0]);
        assert_eq!(star_widths(379.0, 2, 1.0), vec![190.0, 189.0]);
        assert_eq!(star_widths(264.0, 2, 1.0), vec![132.0, 132.0]);
        let w = star_widths(281.0, 3, 1.5);
        assert!((w.iter().sum::<f32>() - 281.0).abs() <= 0.5 && w.iter().all(|x| (x * 1.5).fract() == 0.0), "{w:?}");
    }
}
