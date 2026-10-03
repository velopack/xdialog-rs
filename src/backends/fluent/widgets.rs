//! Fluent theme widgets: the button (Button / AccentButtonStyle), the progress bar (ProgressBar),
//! the severity icon (InfoBar-style) and the look of the body scroll bar (ScrollBar).

use std::rc::Rc;

use super::tokens::{ButtonColors, FluentTokens};
use crate::backends::draw::{LineCap, Point, Rect, Size, Vec2};
use crate::backends::gui::anim::{Easing, Transition};
use crate::backends::gui::text::TextBlock;
use crate::backends::gui::theme::{button_id, ButtonInteraction, DialogView, ProgressView};
use crate::backends::gui::ui::{centered, Id, ScrollBarSpec, Ui};
use crate::model::XDialogIcon;

/// Button height (padding 5/6 + one text line + 1 px borders).
pub(crate) const BUTTON_H: f64 = 32.0;
/// ControlCornerRadius.
const CORNER: f64 = 4.0;
/// Button background fade (standard and accent). Slower than WinUI's 83 ms
/// (ControlFasterAnimationDuration, standard only) so the state change reads.
const FADE: Transition = Transition::linear(0.15);
/// Determinate value changes (ControlNormalAnimationDuration, fast-out-slow-in).
const PROGRESS_VALUE: Transition = Transition::new(0.25, Easing::FLUENT_FAST_OUT_SLOW_IN);
/// Indicator height (WinUI's ProgressBarMinHeight is 3; 4 lets the pill ends read as round at
/// 100 % scale) and indeterminate loop length.
pub(crate) const PROGRESS_H: f64 = 4.0;
const INDETERMINATE_LOOP: f64 = 2.0;
/// Icon box (the 32 px InfoBar glyph composite).
pub(crate) const ICON_SIZE: f64 = 32.0;

// ------------------------------------------------------------------------------------------------
// Button
// ------------------------------------------------------------------------------------------------

/// A ContentDialog command button in `rect`: standard (DefaultButtonStyle) or accent
/// (AccentButtonStyle).
///
/// - Both styles: the background fades (`FADE`); label and border switch instantly.
/// - Standard border = ControlElevationBorderBrush (rest, pointer-over) or flat
///   ControlStrokeColorDefault (pressed).
/// - Accent: no border when pressed.
/// - Pointer-over = `hovered` (no hover while another button is pressed, like WinUI's pointer
///   capture); pressed = pointer held AND inside (the look drops when dragged off), or Space held.
/// - Focus visual (1 px inner + 2 px outer ring) only with keyboard focus visibility.
pub(crate) fn button(ui: &mut Ui<'_>,
                     rect: Rect,
                     index: usize,
                     label: &Rc<TextBlock>,
                     accent: bool,
                     view: &DialogView<'_>,
                     tk: &FluentTokens) {
    let st = ButtonInteraction::interact(ui, rect, index, view);
    let (std, acc) = if st.pointer_down && st.contains_pointer || st.key_pressed {
        (tk.std_pressed, tk.acc_pressed)
    } else if st.hovered {
        (tk.std_hover, tk.acc_hover)
    } else {
        (tk.std_rest, tk.acc_rest)
    };
    // Both fills keep tracking (snapping while the other style shows), so a button that changes
    // style (accent follows focus) shows its current look at once.
    let std_fill = ui.animate(button_id(index).with("fluent.bg"), std.fill, if accent { Transition::INSTANT } else { FADE });
    let acc_fill = ui.animate(button_id(index).with("fluent.acc_bg"), acc.fill, if accent { FADE } else { Transition::INSTANT });
    let colors = if accent { ButtonColors { fill: acc_fill, ..acc } } else { ButtonColors { fill: std_fill, ..std } };
    paint_box(ui, rect, &colors, !accent && tk.std_elevation_top);
    if st.focus_visible {
        ui.stroke_rect(rect.inflate(0.5, 0.5), CORNER + 0.5, 1.0, tk.focus_inner);
        ui.stroke_rect(rect.inflate(2.0, 2.0), CORNER + 2.0, 2.0, tk.focus_outer);
    }
    ui.text(label, centered(rect, label.size).origin(), colors.text);
}

/// Button background: a 1 px border (`stroke`, with the `stroke_2` elevation edge at the top or
/// bottom) around the fill, or just the fill when borderless.
fn paint_box(ui: &mut Ui<'_>, r: Rect, c: &ButtonColors, elevation_top: bool) {
    if c.borderless {
        ui.fill_rect(r, CORNER, c.fill);
        return;
    }
    ui.fill_rect(r, CORNER, c.stroke_2);
    let sides = if elevation_top { Rect { y0: r.y0 + 1.0, ..r } } else { Rect { y1: r.y1 - 1.0, ..r } };
    ui.fill_rect(sides, CORNER, c.stroke);
    ui.fill_rect(r.inset(-1.0), CORNER - 1.0, c.fill);
}

// ------------------------------------------------------------------------------------------------
// Progress bar
// ------------------------------------------------------------------------------------------------

/// WinUI ProgressBar in `r` (`PROGRESS_H` tall). Determinate: 1 px track
/// (ControlStrongStrokeColorDefault) centred in the row + pill indicator from the left.
/// Indeterminate: two accent bars sweeping in a 2 s loop (ProgressBar.xaml storyboard), phase
/// origin = when the bar became indeterminate.
pub(crate) fn progress(ui: &mut Ui<'_>, r: Rect, progress: ProgressView, tk: &FluentTokens) {
    let id = Id::new("fluent.progress");
    let pill = |ui: &mut Ui<'_>, rect: Rect| ui.fill_rect_unsnapped(rect, rect.height() / 2.0, tk.progress_fill);
    match progress {
        ProgressView::Determinate { value } => {
            let v = ui.animate(id, value.clamp(0.0, 1.0), PROGRESS_VALUE) as f64;
            let cy = r.center().y;
            ui.fill_rect(Rect::new(r.x0, cy - 0.5, r.x1, cy + 0.5), 0.5, tk.progress_track);
            let w = r.width() * v;
            if w > 0.0 {
                pill(ui, Rect::from_origin_size(r.origin(), Size::new(w, PROGRESS_H)));
            }
        }
        ProgressView::Indeterminate { since, .. } => {
            // WinUI collapses the determinate indicator (width 0) while indeterminate: a later
            // value grows from 0, never from the stale pre-indeterminate value.
            ui.animate(id, 0.0f32, Transition::INSTANT);
            ui.request_smooth_frame();
            let t = (ui.time() - since).rem_euclid(INDETERMINATE_LOOP) as f32;
            // Clamp each bar to the track (not clip it), so it keeps round ends while it slides
            // in and out.
            for (x, w) in indeterminate_bars(t, r.width() as f32) {
                let (x0, x1) = (x.max(0.0) as f64, ((x + w) as f64).min(r.width()));
                if x1 > x0 {
                    pill(ui, Rect::new(r.x0 + x0, r.y0, r.x0 + x1, r.y1));
                }
            }
        }
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

/// Symbol geometry (px of the 32 px box, relative to the circle centre): dots at y -5.5 (i) /
/// +5.5 (!), 2 px bars (i: -0.94..5.94, !: -5.94..0.94), X = two 2 px diagonals in a 10 px square.
const BAR_ENDS: (f64, f64) = (0.94, 5.94);
const DOT_Y: f64 = 5.5;
const DOT_R: f64 = 1.6;
const X_HALF: f64 = 4.95;

/// InfoBar-style severity icon in the 32 px box at `origin`: a filled circle (radius 15 centred at
/// (15, 17)) in the severity colour with the symbol in TextFillColorInverse.
pub(crate) fn icon(ui: &mut Ui<'_>, origin: Point, icon: &XDialogIcon, tk: &FluentTokens) {
    let circle = match icon {
        XDialogIcon::Information => tk.sev_info,
        XDialogIcon::Warning => tk.sev_warning,
        XDialogIcon::Error => tk.sev_error,
        // `Custom` is drawn by the theme itself (a bigger image, top left).
        XDialogIcon::None | XDialogIcon::Custom => return,
    };
    let c = origin + Vec2::new(15.0, 17.0);
    let p = |x: f64, y: f64| c + Vec2::new(x, y);
    // The symbol colour is translucent in dark mode: pre-blend it over the circle so overlapping
    // parts (the X's crossing) don't darken.
    let g = circle.blend(tk.sev_glyph);
    ui.circle(c, 15.0, circle);
    let (b0, b1) = BAR_ENDS;
    let bar = |ui: &mut Ui<'_>, y0: f64, y1: f64| ui.fill_rect_unsnapped(Rect::from_points(p(-1.0, y0), p(1.0, y1)), 0.0, g);
    match icon {
        XDialogIcon::Information => {
            ui.circle(p(0.0, -DOT_Y), DOT_R, g);
            bar(ui, -b0, b1);
        }
        XDialogIcon::Warning => {
            bar(ui, -b1, b0);
            ui.circle(p(0.0, DOT_Y), DOT_R, g);
        }
        XDialogIcon::Error => {
            let a = X_HALF;
            ui.line(p(-a, -a), p(a, a), 2.0, LineCap::Butt, g);
            ui.line(p(a, -a), p(-a, a), 2.0, LineCap::Butt, g);
        }
        XDialogIcon::None | XDialogIcon::Custom => {}
    }
}

// ------------------------------------------------------------------------------------------------
// Scroll bar
// ------------------------------------------------------------------------------------------------

/// WinUI's overlay scroll bar (ScrollBar), in `FluentTokens::scroll_thumb`.
pub(crate) const SCROLL_BAR: ScrollBarSpec = ScrollBarSpec { thin: 2.0,
                                                             wide: 6.0,
                                                             inset: 2.0,
                                                             end_inset: 0.0,
                                                             min_thumb: 12.0,
                                                             fade: Transition::linear(0.1),
                                                             max_radius: Some(3.0) };

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
        let end = indeterminate_bars(1.9999, w);
        assert_eq!(end.len(), 1);
        assert!((end[0].0 - 0.996 * w).abs() < 0.5, "{end:?}");
    }
}
