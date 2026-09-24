//! Fluent theme widgets: `FluentButton`
//! (Button / AccentButtonStyle), `FluentProgress` (ProgressBar) and `FluentIcon` (InfoBar-style
//! severity icon, drawn procedurally). Hover/press/focus come from egui through
//! `ButtonInteraction`; animations are widget-local `anim::animate*` tweens on the dialog clock.

use egui::{Color32, Id, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, Widget};

use super::tokens::{ButtonColors, FluentTokens};
use crate::backends::egui_core::anim::{self, Easing, Transition};
use crate::backends::egui_core::paint_util::{self, CornerStyle, EdgeColors};
use crate::backends::egui_core::text::TextBlock;
use crate::backends::egui_core::theme::{ButtonInteraction, DialogView, ProgressView};
use crate::model::XDialogIcon;

/// Button height (padding 5/6 + 19 px text line + 1 px borders; measured 32).
pub(crate) const BUTTON_H: f32 = 32.0;
/// ControlCornerRadius is 4, but the composition renderer's anti-aliased corners depart from the
/// edges earlier and still reach the diagonal about where a 5 px arc does: quadratic corners
/// (control point at the rect corner) of this extent minimise the pixel error against the captures.
const BUTTON_CORNER: f32 = 6.25;
/// Focus visual (FocusVisualPrimary/SecondaryThickness 2/1, around the button): circular arcs.
/// Nominally `4 + offset`; the reference arcs are rounder, fitted as a larger base radius per ring.
const FOCUS_INNER_RADIUS: f32 = 4.5;
const FOCUS_OUTER_RADIUS: f32 = 4.75;
/// Label top inside the button: 1 px border + 5 px padding.
const LABEL_TOP: f32 = 6.0;
/// ControlFasterAnimationDuration: the standard button's BrushTransition (background only).
const FASTER: Transition = Transition::linear(0.083);
/// The BrushTransition starts on the next composition frame after the state change: the
/// captured ramps (`anim/transition_*/timing.csv`) begin 20-30 ms after the input.
const TRANSITION_DELAY: f64 = 0.025;
/// Determinate value changes (ControlNormalAnimationDuration, fast-out-slow-in).
const PROGRESS_VALUE: Transition = Transition::new(0.25, Easing::FLUENT_FAST_OUT_SLOW_IN);
/// ProgressBar height (ProgressBarMinHeight) and indeterminate loop length.
pub(crate) const PROGRESS_H: f32 = 3.0;
const INDETERMINATE_LOOP: f64 = 2.0;
/// Icon box (the 32 px InfoBar glyph composite).
pub(crate) const ICON_SIZE: f32 = 32.0;

// ------------------------------------------------------------------------------------------------
// Text
// ------------------------------------------------------------------------------------------------

/// Glyph coverage -> alpha of the (single, context-wide) font atlas, fitted to DirectWrite's
/// grayscale AA ink weight for DARK-on-light text (ink sum within ~2% of the captures).
pub(crate) const TEXT_GAMMA: f32 = 1.05;
/// DirectWrite's contrast/gamma handling makes LIGHT-on-dark text heavier than the same coverage
/// dark-on-light. The atlas gamma can't depend on the text, so light-on-dark text is painted a
/// second time at this opacity (`a' = a + k·a·(1 - a)`: cores stay exact, partial edge pixels
/// get heavier), fitted to the white-on-dark body text and the white-on-accent labels.
const LIGHT_TEXT_BOOST: f32 = 0.75;

fn luminance(c: Color32) -> f32 {
    0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32
}

/// Paint `block` in `color` over the (opaque) `bg`, with the text-polarity dependent weight.
/// The colour is pre-blended over `bg` first, so the boost pass only thickens the anti-aliased
/// edges and leaves the glyph cores of translucent text colours exact.
pub(crate) fn paint_text(painter: &egui::Painter, block: &TextBlock, top_left: Pos2, color: Color32, bg: Color32) {
    let solid = crate::backends::egui_core::color::over(color, bg);
    block.paint(painter, top_left, solid);
    if luminance(solid) > luminance(bg) {
        block.paint(painter, top_left, solid.gamma_multiply(LIGHT_TEXT_BOOST));
    }
}

/// A [`TextBlock`] as a widget (allocates `block.size`) painted with [`paint_text`].
pub(crate) struct FluentText<'a> {
    pub block: &'a TextBlock,
    pub color: Color32,
    pub bg: Color32,
}

impl Widget for FluentText<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, response) = ui.allocate_exact_size(self.block.size, Sense::hover());
        if ui.is_rect_visible(rect) {
            paint_text(ui.painter(), self.block, rect.min, self.color, self.bg);
        }
        response
    }
}

// ------------------------------------------------------------------------------------------------
// Button
// ------------------------------------------------------------------------------------------------

/// A ContentDialog command button: standard (DefaultButtonStyle) or accent (AccentButtonStyle).
///
/// - Standard: the background fades over 83 ms (linear BrushTransition, starting one composition
///   frame after the state change); label and border switch instantly. Fill inset 1 px (InnerBorderEdge); border = ControlElevationBorderBrush (rest,
///   pointer-over) or flat ControlStrokeColorDefault (pressed, disabled).
/// - Accent: everything instant (the states differ only by brush opacity, which BrushTransition
///   doesn't animate); fill under the border (OuterBorderEdge); no border when pressed/disabled.
/// - Pointer-over = `hovered` (egui suppresses hover while another widget is pressed, like WinUI's
///   pointer capture); pressed = pointer held AND inside (the look drops when dragged off), or
///   Space held.
/// - Focus visual (1 px inner + 2 px outer ring) only with keyboard focus visibility.
pub(crate) struct FluentButton<'a> {
    pub index: usize,
    pub label: &'a TextBlock,
    pub width: f32,
    pub accent: bool,
    pub view: &'a DialogView<'a>,
    pub tk: &'a FluentTokens,
}

impl Widget for FluentButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(self.width, BUTTON_H), Sense::hover());
        let st = ButtonInteraction::interact(ui, rect, self.index, self.view);
        let tk = self.tk;
        let pressed = st.pointer_down && st.contains_pointer || st.key_pressed;
        // 0 rest, 1 pointer-over, 2 pressed, 3 disabled.
        let state: u8 = if st.disabled {
            3
        } else if pressed {
            2
        } else if st.hovered {
            1
        } else {
            0
        };
        let looks = |k: u8| match k {
            3 => (tk.std_disabled, tk.acc_disabled),
            2 => (tk.std_pressed, tk.acc_pressed),
            1 => (tk.std_hover, tk.acc_hover),
            _ => (tk.std_rest, tk.acc_rest),
        };
        let (std, acc) = looks(state);
        // The standard fill keeps tracking while the button is accent, so a button that loses the
        // accent style (accent follows focus) shows its current standard look at once; it snaps
        // meanwhile (no fade, no repaints for an invisible fill).
        let std_fill = fill_transition(ui.ctx(), st.response.id.with("fluent.bg"), state, std.fill, !self.accent);
        if !ui.is_rect_visible(rect) {
            return st.response;
        }
        let ppp = ui.ctx().pixels_per_point();
        let r = paint_util::snap(rect, ppp);
        let painter = ui.painter();
        let colors = if self.accent {
            paint_accent(painter, r, &acc, ppp);
            acc
        } else {
            let c = ButtonColors { fill: std_fill, ..std };
            paint_standard(painter, r, &c, tk.std_elevation_top, ppp);
            c
        };
        if st.focus_visible {
            // Whole physical pixels at any scale (XAML layout rounding of the thicknesses): e.g.
            // 1 + 3 px at 125 %, 2 + 3 at 150 %.
            let (inner, outer) = (whole_px(1.0, ppp), whole_px(2.0, ppp));
            let ring = |off: f32, w: f32, radius: f32, c| {
                paint_util::stroke_rounded_rect(painter, r.expand(off + w / 2.0), radius + off + w / 2.0, CornerStyle::Circular, w, c, ppp)
            };
            ring(0.0, inner, FOCUS_INNER_RADIUS, tk.focus_inner);
            ring(inner, outer, FOCUS_OUTER_RADIUS, tk.focus_outer);
        }
        // Label: centred in the content box (padding 11 + border 1 on each side), whole pixels
        // (XAML layout rounding).
        let content_w = r.width() - 24.0;
        let x = r.min.x + 12.0 + paint_util::snap_len((content_w - self.label.size.x) / 2.0, ppp);
        let clip = painter.with_clip_rect(r.shrink(1.0).intersect(painter.clip_rect()));
        paint_text(&clip, self.label, Pos2::new(x, r.min.y + LABEL_TOP), colors.text, colors.fill);
        st.response
    }
}

/// The standard button's background BrushTransition: on a visual-state change, a linear
/// [`FASTER`] fade from the displayed colour to the new state's fill, starting
/// [`TRANSITION_DELAY`] after the change (interruptible). Snaps when `animate` is false, and when
/// the state's colour itself changed (appearance/accent change: no fade between palettes).
fn fill_transition(ctx: &egui::Context, id: Id, state: u8, target: Color32, animate: bool) -> Color32 {
    #[derive(Clone, Copy)]
    struct Fade {
        state: u8,
        from: Color32,
        to: Color32,
        start: f64,
    }
    impl Fade {
        fn value(&self, now: f64) -> Color32 {
            let t = ((now - self.start) / FASTER.duration as f64).clamp(0.0, 1.0) as f32;
            crate::backends::egui_core::color::mix(self.from, self.to, FASTER.easing.apply(t))
        }
    }
    let now = ctx.input(|i| i.time);
    let (value, running) = ctx.data_mut(|d| {
                                   let f = d.get_temp_mut_or_insert_with(id, || Fade { state, from: target, to: target, start: now });
                                   if !animate || (f.state == state && f.to != target) {
                                       *f = Fade { state, from: target, to: target, start: now };
                                   } else if f.state != state {
                                       *f = Fade { state, from: f.value(now), to: target, start: now + TRANSITION_DELAY };
                                   }
                                   (f.value(now), now < f.start + FASTER.duration as f64 && f.from != f.to)
                               });
    if running {
        ctx.request_repaint();
    }
    value
}

/// Standard button: fill inset by the 1 px border, border with the elevation edge at the bottom
/// (light) or top (dark).
fn paint_standard(painter: &egui::Painter, r: Rect, c: &ButtonColors, elevation_top: bool, ppp: f32) {
    let bw = whole_px(1.0, ppp);
    paint_util::fill_rounded_rect(painter, r.shrink(bw), BUTTON_CORNER - bw, CornerStyle::Quadratic, c.fill, ppp);
    let (top, bottom) = if elevation_top { (c.stroke_2, c.stroke) } else { (c.stroke, c.stroke_2) };
    border(painter, r, bw, EdgeColors { top, right: c.stroke, bottom, left: c.stroke }, ppp);
}

/// Accent button: fill under the border; the border (if any) has its dark edge at the bottom.
fn paint_accent(painter: &egui::Painter, r: Rect, c: &ButtonColors, ppp: f32) {
    paint_util::fill_rounded_rect(painter, r, BUTTON_CORNER, CornerStyle::Quadratic, c.fill, ppp);
    if !c.borderless {
        border(painter, r, whole_px(1.0, ppp), EdgeColors { top: c.stroke, right: c.stroke, bottom: c.stroke_2, left: c.stroke }, ppp);
    }
}

/// `v` logical px rounded to whole physical pixels (at least one).
fn whole_px(v: f32, ppp: f32) -> f32 {
    (v * ppp).round().max(1.0) / ppp
}

/// A border of width `w` inside `r` with the button's quadratic corners, one colour per edge
/// (corners split halfway between the adjacent edges).
fn border(painter: &egui::Painter, r: Rect, w: f32, edges: EdgeColors, ppp: f32) {
    let pts = paint_util::rounded_rect_path(r.shrink(w / 2.0), BUTTON_CORNER - w / 2.0, CornerStyle::Quadratic, ppp);
    // 4 corners x (segs + 1) points, clockwise from the top-right corner.
    let per = pts.len() / 4;
    let half = per / 2;
    let edge = |c0: usize, c1: usize, color: Color32| {
        let mut v: Vec<Pos2> = (half..per).map(|i| pts[c0 * per + i]).collect();
        v.extend((0..=half).map(|i| pts[c1 * per + i]));
        painter.add(Shape::Path(egui::epaint::PathShape { points: v, closed: false, fill: Color32::TRANSPARENT, stroke: egui::epaint::PathStroke::new(w, color) }));
    };
    edge(0, 1, edges.right);
    edge(1, 2, edges.bottom);
    edge(2, 3, edges.left);
    edge(3, 0, edges.top);
}

// ------------------------------------------------------------------------------------------------
// Progress bar
// ------------------------------------------------------------------------------------------------

/// WinUI ProgressBar (3 px). Determinate: 1 px track (ControlStrongStrokeColorDefault) centred
/// in the row + 3 px indicator from the left. Indeterminate: two accent bars sweeping in a 2 s
/// loop (ProgressBar.xaml storyboard), phase origin = when the bar became indeterminate.
pub(crate) struct FluentProgress<'a> {
    pub progress: ProgressView,
    pub width: f32,
    pub tk: &'a FluentTokens,
}

impl Widget for FluentProgress<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::new(self.width, PROGRESS_H), Sense::hover());
        let ctx = ui.ctx().clone();
        let id = Id::new("fluent.progress");
        let ppp = ctx.pixels_per_point();
        let r = paint_util::snap(rect, ppp);
        let painter = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
        let tk = self.tk;
        match self.progress {
            ProgressView::Determinate { value, .. } => {
                let v = anim::animate_f32(&ctx, id, value.clamp(0.0, 1.0), PROGRESS_VALUE);
                let track = Rect::from_min_size(Pos2::new(r.min.x, r.min.y + 1.0), Vec2::new(r.width(), 1.0));
                painter.rect_filled(track, 0.5, tk.progress_track);
                let w = r.width() * v;
                if w > 0.0 {
                    painter.rect_filled(Rect::from_min_size(r.min, Vec2::new(w, PROGRESS_H)), PROGRESS_H / 2.0, tk.progress_fill);
                }
            }
            ProgressView::Indeterminate { since, .. } => {
                // WinUI collapses the determinate indicator (width 0) while indeterminate: a later
                // value grows from 0, never from the stale pre-indeterminate value.
                anim::animate_f32(&ctx, id, 0.0, Transition::INSTANT);
                ctx.request_repaint();
                let t = (ctx.input(|i| i.time) - since).rem_euclid(INDETERMINATE_LOOP) as f32;
                for (x, w) in indeterminate_bars(t, r.width()) {
                    let bar = Rect::from_min_size(Pos2::new(r.min.x + x, r.min.y), Vec2::new(w, PROGRESS_H));
                    painter.rect_filled(bar, PROGRESS_H / 2.0, tk.progress_fill);
                }
            }
        }
        response
    }
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

/// InfoBar-style severity icon (32 px): a filled circle in the severity colour with the symbol in
/// TextFillColorInverse, drawn from primitives (no Segoe Fluent Icons). Proportions fitted to the
/// WinUI captures (InfoBar severity icons at 32 px): circle
/// radius 15 centred at (15, 17) of the 32 px box, 2 px strokes.
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

/// Symbol geometry (px of the 32 px box, relative to the circle centre), measured on the Segoe
/// InfoBar glyph captures: dots ~3.4 x 3.0 (i: centre y -5.5, !: +5.5), 2 px bars (i: -0.94..5.94,
/// !: -5.94..0.94), X = two 45 degree bands (half-width 1.07) clipped to a 10 px square.
const DOT_R: [Vec2; 2] = [Vec2::new(1.7, 1.5), Vec2::new(1.45, 1.47)];
const BAR_ENDS: (f32, f32) = (0.94, 5.94);
const X_HALF: f32 = 4.95;
const X_BAND: f32 = 1.07;
/// DirectWrite renders the dark-on-light (dark theme) symbol's anti-aliased edges lighter: its
/// dots are smaller and the bar ends / X edges about 0.1 px thinner.
const DARK_THIN: f32 = 0.12;

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
    let g = crate::backends::egui_core::color::over(tk.sev_glyph, circle);
    let thin = if tk.dark { DARK_THIN } else { 0.0 };
    let poly = |pts: Vec<Pos2>| painter.add(Shape::convex_polygon(pts, g, Stroke::NONE));
    // `rect_filled` rounds to whole pixels; the partial end pixels of the bars matter here.
    let bar = |y0: f32, y1: f32| {
        let (x0, x1) = (-1.0, 1.0);
        poly(vec![p(x0, y0 + thin), p(x1, y0 + thin), p(x1, y1 - thin), p(x0, y1 - thin)]);
    };
    // epaint's cached small-circle paths are visibly off-centre at r ~ 1.5: tessellate our own.
    let dot = |x: f32, y: f32| {
        let r = DOT_R[usize::from(tk.dark)] * s;
        let centre = p(x, y);
        poly((0..32).map(|k| centre + Vec2::angled(k as f32 * std::f32::consts::TAU / 32.0) * r).collect());
    };
    painter.circle_filled(c, 15.0 * s, circle);
    let (b0, b1) = BAR_ENDS;
    match icon {
        XDialogIcon::Information => {
            dot(0.0, -5.5);
            bar(-b0, b1);
        }
        XDialogIcon::Warning => {
            bar(-b1, b0);
            dot(0.0, 5.5);
        }
        XDialogIcon::Error => {
            // Band |x - y| <= k (resp. |x + y| <= k) clipped to the square |x|, |y| <= a: hexagons.
            let (a, k) = (X_HALF - thin, (X_BAND - 0.6 * thin) * std::f32::consts::SQRT_2);
            poly(vec![p(-a, -a), p(-a + k, -a), p(a, a - k), p(a, a), p(a - k, a), p(-a, -a + k)]);
            poly(vec![p(a, -a), p(a, -a + k), p(-a + k, a), p(-a, a), p(-a, a - k), p(a - k, -a)]);
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
