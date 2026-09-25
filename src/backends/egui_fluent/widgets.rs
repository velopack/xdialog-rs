//! Fluent theme widgets: `FluentButton` (Button / AccentButtonStyle), `FluentProgress`
//! (ProgressBar) and `FluentIcon` (InfoBar-style severity icon, drawn from egui shapes).
//! Hover/press/focus come from egui through `ButtonInteraction`; animations are widget-local
//! `anim::animate*` tweens on the dialog clock.

use egui::{Id, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, Widget};

use super::tokens::{ButtonColors, FluentTokens};
use crate::backends::egui_core::anim::{self, Easing, Transition};
use crate::backends::egui_core::text::TextBlock;
use crate::backends::egui_core::theme::{ButtonInteraction, DialogView, ProgressView};
use crate::model::XDialogIcon;

/// Button height (padding 5/6 + one text line + 1 px borders).
pub(crate) const BUTTON_H: f32 = 32.0;
/// ControlCornerRadius.
const CORNER: f32 = 4.0;
/// ControlFasterAnimationDuration: the standard button's BrushTransition (background only).
const FASTER: Transition = Transition::linear(0.083);
/// Determinate value changes (ControlNormalAnimationDuration, fast-out-slow-in).
const PROGRESS_VALUE: Transition = Transition::new(0.25, Easing::FLUENT_FAST_OUT_SLOW_IN);
/// Indicator height (WinUI's ProgressBarMinHeight is 3; 4 lets the pill ends read as round at
/// 100 % scale) and indeterminate loop length.
pub(crate) const PROGRESS_H: f32 = 4.0;
const INDETERMINATE_LOOP: f64 = 2.0;
/// Icon box (the 32 px InfoBar glyph composite).
pub(crate) const ICON_SIZE: f32 = 32.0;

// ------------------------------------------------------------------------------------------------
// Button
// ------------------------------------------------------------------------------------------------

/// A ContentDialog command button: standard (DefaultButtonStyle) or accent (AccentButtonStyle),
/// as wide as the space the parent `Ui` offers.
///
/// - Standard: the background fades over 83 ms (linear BrushTransition); label and border switch
///   instantly. Border = ControlElevationBorderBrush (rest, pointer-over) or flat
///   ControlStrokeColorDefault (pressed).
/// - Accent: everything instant; no border when pressed.
/// - Pointer-over = `hovered` (egui suppresses hover while another widget is pressed, like WinUI's
///   pointer capture); pressed = pointer held AND inside (the look drops when dragged off), or
///   Space held.
/// - Focus visual (1 px inner + 2 px outer ring) only with keyboard focus visibility.
pub(crate) struct FluentButton<'a> {
    pub index: usize,
    pub label: &'a TextBlock,
    pub accent: bool,
    pub view: &'a DialogView<'a>,
    pub tk: &'a FluentTokens,
}

impl FluentButton<'_> {
    /// Allocate, interact and paint; returns the interaction (see `DialogUiOutput::push_button`).
    pub(crate) fn show(self, ui: &mut Ui) -> ButtonInteraction {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), BUTTON_H), Sense::hover());
        let st = ButtonInteraction::interact(ui, rect, self.index, self.view);
        let tk = self.tk;
        let (std, acc) = if st.pointer_down && st.contains_pointer || st.key_pressed {
            (tk.std_pressed, tk.acc_pressed)
        } else if st.hovered {
            (tk.std_hover, tk.acc_hover)
        } else {
            (tk.std_rest, tk.acc_rest)
        };
        // The standard fill keeps tracking while the button is accent (snapping), so a button that
        // loses the accent style (accent follows focus) shows its current standard look at once.
        let tr = if self.accent { Transition::INSTANT } else { FASTER };
        let fill = anim::animate(ui.ctx(), st.response.id.with("fluent.bg"), std.fill, tr);
        if !ui.is_rect_visible(rect) {
            return st;
        }
        let painter = ui.painter();
        let colors = if self.accent { acc } else { ButtonColors { fill, ..std } };
        // The elevation edge: top for the dark standard button, else bottom.
        paint_box(painter, rect, &colors, !self.accent && tk.std_elevation_top);
        if st.focus_visible {
            painter.rect_stroke(rect, CORNER, Stroke::new(1.0, tk.focus_inner), StrokeKind::Outside);
            painter.rect_stroke(rect.expand(1.0), CORNER + 1.0, Stroke::new(2.0, tk.focus_outer), StrokeKind::Outside);
        }
        self.label.paint(painter, rect.center() - self.label.size / 2.0, colors.text);
        st
    }
}

/// Button background: a 1 px border (`stroke`, with the `stroke_2` elevation edge at the top or
/// bottom) around the fill, or just the fill when borderless.
fn paint_box(painter: &egui::Painter, r: Rect, c: &ButtonColors, elevation_top: bool) {
    if c.borderless {
        painter.rect_filled(r, CORNER, c.fill);
        return;
    }
    painter.rect_filled(r, CORNER, c.stroke_2);
    let mut sides = r;
    if elevation_top {
        sides.min.y += 1.0;
    } else {
        sides.max.y -= 1.0;
    }
    painter.rect_filled(sides, CORNER, c.stroke);
    painter.rect_filled(r.shrink(1.0), CORNER - 1.0, c.fill);
}

// ------------------------------------------------------------------------------------------------
// Progress bar
// ------------------------------------------------------------------------------------------------

/// WinUI ProgressBar (full available width). Determinate: 1 px track
/// (ControlStrongStrokeColorDefault) centred in the row + pill indicator from the left.
/// Indeterminate: two accent bars sweeping in a 2 s loop (ProgressBar.xaml storyboard), phase
/// origin = when the bar became indeterminate.
pub(crate) struct FluentProgress<'a> {
    pub progress: ProgressView,
    pub tk: &'a FluentTokens,
}

impl Widget for FluentProgress<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (r, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), PROGRESS_H), Sense::hover());
        let ctx = ui.ctx().clone();
        let id = Id::new("fluent.progress");
        let painter = ui.painter_at(r);
        let tk = self.tk;
        match self.progress {
            ProgressView::Determinate { value } => {
                let v = anim::animate(&ctx, id, value.clamp(0.0, 1.0), PROGRESS_VALUE);
                let track = Rect::from_min_size(Pos2::new(r.min.x, r.center().y - 0.5), Vec2::new(r.width(), 1.0));
                painter.rect_filled(track, 0.5, tk.progress_track);
                let w = r.width() * v;
                if w > 0.0 {
                    pill(&painter, Rect::from_min_size(r.min, Vec2::new(w, PROGRESS_H)), tk.progress_fill);
                }
            }
            ProgressView::Indeterminate { since, .. } => {
                // WinUI collapses the determinate indicator (width 0) while indeterminate: a later
                // value grows from 0, never from the stale pre-indeterminate value.
                anim::animate(&ctx, id, 0.0f32, Transition::INSTANT);
                anim::request_smooth_frame(&ctx);
                let t = (ctx.input(|i| i.time) - since).rem_euclid(INDETERMINATE_LOOP) as f32;
                // Clamp each bar to the track (not clip it), so it keeps round ends while it slides
                // in and out.
                for (x, w) in indeterminate_bars(t, r.width()) {
                    let (x0, x1) = (x.max(0.0), (x + w).min(r.width()));
                    if x1 > x0 {
                        pill(&painter, Rect::from_x_y_ranges(r.min.x + x0..=r.min.x + x1, r.y_range()), tk.progress_fill);
                    }
                }
            }
        }
        response
    }
}

/// A fully rounded bar, not snapped to whole pixels, so moving ends glide instead of stepping.
fn pill(painter: &egui::Painter, rect: Rect, color: egui::Color32) {
    painter.add(egui::epaint::RectShape::filled(rect, rect.height() / 2.0, color).with_round_to_pixels(false));
}

/// `(x offset, width)` of the visible indeterminate bars at loop time `t` (0..2 s) for a bar of
/// width `w` (ProgressBar.xaml IndeterminateStoryboard; widths from ProgressBar.cpp).
fn indeterminate_bars(t: f32, w: f32) -> Vec<(f32, f32)> {
    let ease = Easing::FLUENT_PROGRESS;
    let mut out = Vec::with_capacity(2);
    // Bar 1: 0.4 w, -0.4 w -> 1.2 w over 0..1.5 s, then held off-screen.
    if t < 1.5 {
        let x = -0.4 * w + 1.6 * w * ease.apply(t / 1.5);
        out.push((x, 0.4 * w));
    }
    // Bar 2: 0.6 w, parked at -0.9 w until 0.75 s, then -> 0.996 w at 2.0 s.
    if t >= 0.75 {
        let x = -0.9 * w + 1.896 * w * ease.apply((t - 0.75) / 1.25);
        out.push((x, 0.6 * w));
    }
    out
}

// ------------------------------------------------------------------------------------------------
// Severity icon
// ------------------------------------------------------------------------------------------------

/// InfoBar-style severity icon (32 px): a filled circle (radius 15 centred at (15, 17)) in the
/// severity colour with the symbol in TextFillColorInverse, drawn from egui shapes.
pub(crate) struct FluentIcon<'a> {
    pub icon: &'a XDialogIcon,
    pub tk: &'a FluentTokens,
}

impl Widget for FluentIcon<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(ICON_SIZE), Sense::hover());
        if ui.is_rect_visible(rect) {
            paint_icon(ui.painter(), rect, self.icon, self.tk);
        }
        response
    }
}

/// Symbol geometry (px of the 32 px box, relative to the circle centre): dots at y -5.5 (i) /
/// +5.5 (!), 2 px bars (i: -0.94..5.94, !: -5.94..0.94), X = two 2 px diagonals in a 10 px square.
const BAR_ENDS: (f32, f32) = (0.94, 5.94);
const DOT_Y: f32 = 5.5;
const DOT_R: f32 = 1.6;
const X_HALF: f32 = 4.95;

fn paint_icon(painter: &egui::Painter, rect: Rect, icon: &XDialogIcon, tk: &FluentTokens) {
    let s = rect.width() / ICON_SIZE;
    let c = rect.min + Vec2::new(15.0, 17.0) * s;
    let p = |x: f32, y: f32| c + Vec2::new(x, y) * s;
    let circle = match icon {
        XDialogIcon::Information => tk.sev_info,
        XDialogIcon::Warning => tk.sev_warning,
        XDialogIcon::Error => tk.sev_error,
        XDialogIcon::None => return,
    };
    // The symbol colour is translucent in dark mode: pre-blend it over the circle so overlapping
    // parts (the X's crossing) don't darken.
    let g = circle.blend(tk.sev_glyph);
    painter.circle_filled(c, 15.0 * s, circle);
    let (b0, b1) = BAR_ENDS;
    let bar = |y0: f32, y1: f32| painter.rect_filled(Rect::from_min_max(p(-1.0, y0), p(1.0, y1)), 0.0, g);
    match icon {
        XDialogIcon::Information => {
            painter.circle_filled(p(0.0, -DOT_Y), DOT_R * s, g);
            bar(-b0, b1);
        }
        XDialogIcon::Warning => {
            bar(-b1, b0);
            painter.circle_filled(p(0.0, DOT_Y), DOT_R * s, g);
        }
        XDialogIcon::Error => {
            let (a, stroke) = (X_HALF, Stroke::new(2.0 * s, g));
            painter.line_segment([p(-a, -a), p(a, a)], stroke);
            painter.line_segment([p(a, -a), p(-a, a)], stroke);
        }
        XDialogIcon::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indeterminate_bars_follow_the_storyboard() {
        let w = 300.0;
        // t = 0: bar 1 starts fully left of the track, bar 2 not yet started.
        assert_eq!(indeterminate_bars(0.0, w), vec![(-120.0, 120.0)]);
        // Both visible between 0.75 and 1.5 s.
        assert_eq!(indeterminate_bars(1.0, w).len(), 2);
        // Bar 2 ends at 0.996 w.
        let end = indeterminate_bars(1.9999, w);
        assert_eq!(end.len(), 1);
        assert!((end[0].0 - 0.996 * w).abs() < 0.5, "{end:?}");
    }
}
