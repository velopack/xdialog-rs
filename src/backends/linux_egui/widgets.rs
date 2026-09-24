//! The skia-look widgets: text label, button, progress bar (the icon is in `icons.rs`).
//!
//! Every widget carries the exact (fractional) logical rect the skia layout algorithm computed
//! and paints there, instead of an egui-allocated rect (egui rounds allocations to 1/32 pt, skia
//! positions are sums of `1.2 * size` line pitches). The rect is still registered with the ui.

use egui::{Color32, Id, Pos2, Rect, Response, Sense, Ui, Vec2, Widget};

use super::tokens::*;
use crate::backends::egui_core::anim::{self, Easing, Lerp, Transition};
use crate::backends::egui_core::color::{self, luma601};
use crate::backends::egui_core::paint_util::{self, CornerStyle};
use crate::backends::egui_core::text::TextBlock;
use crate::backends::egui_core::theme::{ButtonInteraction, DialogView, ProgressView};

/// skia button colour fade: 150 ms linear (mina `animator!` default easing).
const FADE: Transition = Transition::linear(0.15);
/// skia progress value animation: 300 ms OutCubic (mina's OutCubic evaluates exactly
/// `1 - (1 - t)^3`).
const VALUE_ANIM: Transition = Transition::new(0.3, Easing::OutCubic);
/// Indeterminate capsule cycle (s) and length factor (`INDETERMINATE_CYCLE`, `_STRETCH`).
const CYCLE: f64 = 3.0;
const STRETCH: f32 = 0.45;
/// skia samples the indeterminate timeline BEFORE advancing it by the tick's dt, so a frame shows
/// the capsule one 16 ms tick late (measured: `progress_indeterminate_*.csv`).
const INDETERMINATE_LAG: f64 = 0.016;
/// Extra label coats for light-on-fill button text in the light style (see [`paint_label`]).
const LIGHT_ON_FILL_COATS: f32 = 1.8;
/// cosmic-text puts the baseline of a `1.2 x size` line box a fraction of a pixel lower than
/// epaint's half-leading placement; epaint rounds each line's galley to whole pixels, so the lines
/// whose fractional top (`k * 16.8`, `k * 21.6`) sits near .5 landed one pixel lower in skia.
/// Shifting the rounding threshold reproduces skia's line rows (measured over the static
/// captures: text error 1.8 -> 0.74 light, 1.7 -> 0.47 dark; the good window is -0.30..-0.21).
const TEXT_Y_BIAS: f32 = -0.26;

/// Top-left where skia paints a text block whose layout box starts at `pos`: the origin is
/// rounded to whole physical pixels (text.rs:255-256).
fn text_origin(pos: Pos2, ppp: f32) -> Pos2 {
    paint_util::snap_pos(pos, ppp) + Vec2::new(0.0, TEXT_Y_BIAS)
}

// ------------------------------------------------------------------------------------------------
// Label
// ------------------------------------------------------------------------------------------------

/// A wrapped title/body paragraph at its layout rect (skia `SkiaLabel`).
pub(crate) struct SkiaLabel<'a> {
    pub block: &'a TextBlock,
    pub rect: Rect,
    pub color: Color32,
}

impl Widget for SkiaLabel<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let response = ui.allocate_rect(self.rect, Sense::hover());
        if ui.is_rect_visible(self.rect) {
            let ppp = ui.ctx().pixels_per_point();
            let origin = text_origin(self.rect.min, ppp);
            // cosmic-text aligns right-to-left lines to the END of the buffer, i.e. the right edge
            // of the text column, not of the widest line (`TextBlock::paint`); left-to-right lines
            // start at the column's left edge.
            for line in &self.block.lines {
                let mut pos = origin + line.offset;
                if line.rtl {
                    pos.x += self.rect.width() - line.width;
                }
                ui.painter().galley(pos, line.galley.clone(), self.color);
            }
        }
        response
    }
}

// ------------------------------------------------------------------------------------------------
// Button
// ------------------------------------------------------------------------------------------------

impl Lerp for ButtonLook {
    /// Per-channel u8 interpolation of the three colours (mina interpolated u8 fields).
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        ButtonLook { border: Color32::lerp(&a.border, &b.border, t),
                     fill: Color32::lerp(&a.fill, &b.fill, t),
                     text: Color32::lerp(&a.text, &b.text, t) }
    }
}

/// Outlined rounded button (skia `SkiaButton`): quadratic corners r = 6, a 2 px border centred
/// on the edge, the label centred. State priority `Pressed > Hovered > Focused > Idle`, every
/// change fades all three colours linearly over 150 ms from the displayed value.
pub(crate) struct SkiaButton<'a> {
    pub index: usize,
    pub rect: Rect,
    pub label: &'a TextBlock,
    pub view: &'a DialogView<'a>,
    pub tk: &'a LinuxTokens,
    /// skia hides the focus border while the pointer is over any button (see `focus_suppressed`).
    pub focus_suppressed: bool,
}

impl SkiaButton<'_> {
    /// Interact + paint; returns the interaction so the dialog can report pointer activation.
    pub(crate) fn show(self, ui: &mut Ui) -> ButtonInteraction {
        let rect = self.rect;
        ui.advance_cursor_after_rect(rect);
        let st = ButtonInteraction::interact(ui, rect, self.index, self.view);
        let tk = self.tk;
        // skia drew the focus ring whether or not the window was active. `Response::has_focus`
        // (and so `st.focus_visible`) is false while the window is inactive; egui's focus memory
        // keeps the focused widget, so read that instead (not in the measure pass, whose look
        // must stay the idle seed).
        let focused = !self.view.frame.sizing && self.view.frame.focus_visible && ui.ctx().memory(|m| m.has_focus(st.response.id));
        let target = if st.disabled {
            tk.idle
        } else if st.pointer_down || st.key_pressed {
            // skia keeps the pressed look while the pointer is dragged off the button.
            tk.pressed
        } else if st.contains_pointer && ui.input(|i| i.pointer.latest_pos().is_some()) {
            // skia hovers by geometry, even while another button is held. egui still reports
            // `contains_pointer` on the pass that delivers `PointerGone` (it clears the interact
            // position a pass later), and nothing may request that pass: check the pointer.
            tk.hover
        } else if focused && !self.focus_suppressed {
            tk.focused
        } else {
            tk.idle
        };
        // skia creates every button's animator in the idle state and pre-renders the idle look,
        // so the button focused on open fades its focus look in over the first 150 ms (core snaps
        // tweens on the first real frame): seed the tween with the idle look once.
        let shown_key = st.response.id.with("linux.shown");
        if !self.view.frame.sizing && !ui.ctx().data(|d| d.get_temp::<bool>(shown_key).unwrap_or(false)) {
            ui.ctx().data_mut(|d| d.insert_temp(shown_key, true));
            anim::animate(ui.ctx(), st.response.id, tk.idle, FADE);
        }
        let look = anim::animate(ui.ctx(), st.response.id, target, FADE);
        if ui.is_rect_visible(rect.expand(BUTTON_BORDER)) {
            let ppp = ui.ctx().pixels_per_point();
            let painter = ui.painter();
            // Fill, then the border centred on the edge (skia button.rs).
            paint_util::fill_rounded_rect(painter, rect, BUTTON_RADIUS, CornerStyle::Quadratic, look.fill, ppp);
            paint_util::stroke_rounded_rect(painter, rect, BUTTON_RADIUS, CornerStyle::Quadratic, BUTTON_BORDER, look.border, ppp);
            // Label: the `lines * 1.2 * 14` box centred in the button, origin rounded (button.rs:213).
            let pos = rect.min + (rect.size() - self.label.size) / 2.0;
            let origin = text_origin(pos, ppp);
            paint_label(painter, self.label, origin, look, tk);
        }
        st
    }
}

/// The button label. skia drew it identically in both styles, but the text transfer curves here
/// are fitted per style for the body text (`LIGHT_TEXT_GAMMA`: dark on light / light on dark).
/// The light curve thins white text on the accent fill (hover/pressed) by about 10 % of its ink,
/// so extra coats are added in proportion to how much lighter than the fill the text is (0 for the
/// dark-on-white idle/focused looks); 1.8 coats bring the hover/pressed labels to 0.97-1.01 of
/// skia's ink and lower their per-pixel error. The dark style's white-on-accent labels are about
/// 5 % heavier (idle labels +3 %) at their anti-aliased edges; thinning them with a translucent
/// coat evens the ink but raises the per-pixel error (glyph cores are exact), so they are left.
fn paint_label(painter: &egui::Painter, label: &TextBlock, origin: Pos2, look: ButtonLook, tk: &LinuxTokens) {
    label.paint(painter, origin, look.text);
    if tk.dark {
        return;
    }
    let on_fill = ((luma601(look.text) - luma601(look.fill)) / 255.0 * 2.0).clamp(0.0, 1.0);
    let mut extra = on_fill * LIGHT_ON_FILL_COATS;
    while extra > 0.0 {
        label.paint(painter, origin, color::with_alpha(look.text, extra.min(1.0)));
        extra -= 1.0;
    }
}

impl Widget for SkiaButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show(ui).response
    }
}

/// Button width for a label: natural label width + 2 x 24 (no minimum).
pub(crate) fn button_size(label: &TextBlock) -> Vec2 {
    Vec2::new(label.size.x + 2.0 * BUTTON_PAD_X, BUTTON_H)
}

// ------------------------------------------------------------------------------------------------
// Progress
// ------------------------------------------------------------------------------------------------

/// The progress bar (skia `SkiaProgressBar`): a radius-2 track + bar animating to each new value
/// over 300 ms OutCubic; indeterminate = a pill track with a "stretchy capsule" on a 3 s loop.
pub(crate) struct SkiaProgress<'a> {
    pub progress: ProgressView,
    pub rect: Rect,
    pub tk: &'a LinuxTokens,
}

/// Stable id: the value tween must survive indeterminate phases (skia starts the next value
/// animation from the last determinate bar end).
fn progress_id() -> Id {
    Id::new("linux.progress.value")
}

impl Widget for SkiaProgress<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let response = ui.allocate_rect(self.rect, Sense::hover());
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let painter = ui.painter();
        let (rect, tk) = (self.rect, self.tk);
        match self.progress {
            ProgressView::Determinate { value, .. } => {
                let v = anim::animate_f32(&ctx, progress_id(), value.clamp(0.0, 1.0), VALUE_ANIM).clamp(0.0, 1.0);
                paint_util::fill_rounded_rect(painter, rect, PROGRESS_RADIUS, CornerStyle::Quadratic, tk.progress_bg, ppp);
                let bar_w = v * rect.width();
                if bar_w > 0.0 {
                    let bar = Rect::from_min_size(rect.min, Vec2::new(bar_w, rect.height()));
                    paint_util::fill_rounded_rect(painter, bar, PROGRESS_RADIUS, CornerStyle::Quadratic, tk.progress_fg, ppp);
                }
            }
            ProgressView::Indeterminate { restarted_at, .. } => {
                // skia freezes a running value animation while indeterminate.
                anim::stop::<f32>(&ctx, progress_id());
                let r = rect.height() / 2.0;
                paint_util::fill_rounded_rect(painter, rect, r, CornerStyle::Quadratic, tk.progress_bg, ppp);
                let elapsed = (ui.input(|i| i.time) - restarted_at - INDETERMINATE_LAG).max(0.0);
                let pos = capsule_pos((elapsed.rem_euclid(CYCLE) / CYCLE) as f32);
                let (w, d) = (rect.width(), rect.height());
                let len = d + STRETCH * (w - d);
                let cx = (d - len / 2.0) + pos * (w - 2.0 * d + len);
                let left = (cx - len / 2.0).max(0.0);
                let right = (cx + len / 2.0).min(w);
                if right > left {
                    let cap = Rect::from_min_max(Pos2::new(rect.left() + left, rect.top()), Pos2::new(rect.left() + right, rect.bottom()));
                    paint_util::fill_rounded_rect(painter, cap, r, CornerStyle::Quadratic, tk.progress_fg, ppp);
                }
                ctx.request_repaint();
            }
        }
        response
    }
}

/// Capsule travel position 0..1 at normalized cycle time `n` (skia `INDETERMINATE_TIMELINE`):
/// 0-40 % sweep right, 40-50 % hold, 50-90 % sweep back, 90-100 % hold. mina's "InOutCubic" is a
/// cubic-bezier(0.65, 0, 0.35, 1) evaluated at the curve PARAMETER (lyon `y(t)`), i.e. exactly
/// smoothstep `3t^2 - 2t^3` per segment.
pub(crate) fn capsule_pos(n: f32) -> f32 {
    let smooth = |t: f32| {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    if n < 0.4 {
        smooth(n / 0.4)
    } else if n < 0.5 {
        1.0
    } else if n < 0.9 {
        1.0 - smooth((n - 0.5) / 0.4)
    } else {
        0.0
    }
}
