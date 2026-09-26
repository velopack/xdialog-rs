//! The Ubuntu theme's widgets: the outlined button, the progress bar and the icons.

use std::rc::Rc;

use super::tokens::*;
use crate::backends::draw::color::rgb;
use crate::backends::draw::{Color, LineCap, Point, Rect, Size, Vec2};
use crate::backends::gui::anim::{Easing, Lerp, Transition};
use crate::backends::gui::text::TextBlock;
use crate::backends::gui::theme::{button_id, ButtonInteraction, DialogView, ProgressView};
use crate::backends::gui::ui::{caps_centered, Id, Ui};
use crate::model::XDialogIcon;

/// Button colour fade: 150 ms linear.
const FADE: Transition = Transition::linear(0.15);
/// Progress value animation: 300 ms OutCubic.
const VALUE_ANIM: Transition = Transition::new(0.3, Easing::OutCubic);
/// Indeterminate capsule cycle (s) and its length as a fraction of the free track.
const CYCLE: f64 = 3.0;
const STRETCH: f64 = 0.45;

// ------------------------------------------------------------------------------------------------
// Button
// ------------------------------------------------------------------------------------------------

impl Lerp for ButtonLook {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        ButtonLook { border: a.border.lerp_to_gamma(b.border, t),
                     fill: a.fill.lerp_to_gamma(b.fill, t),
                     text: a.text.lerp_to_gamma(b.text, t) }
    }
}

/// Natural width of a button with `label`: label + 2 x 24 (no minimum).
pub(crate) fn button_width(label: &TextBlock) -> f64 {
    label.size.width + 2.0 * BUTTON_PAD_X
}

/// Outlined rounded button in `rect`: radius 6, a 2 px border centred on the edge, the label
/// centred (capitals centred vertically). State priority `Pressed > Hovered > Focused > Idle`; every change fades all three
/// colours linearly over 150 ms from the displayed value. The focus border is hidden while
/// `focus_suppressed` (the pointer is over a button).
pub(crate) fn button(ui: &mut Ui<'_>,
                     rect: Rect,
                     index: usize,
                     label: &Rc<TextBlock>,
                     view: &DialogView<'_>,
                     tk: &UbuntuTokens,
                     focus_suppressed: bool)
                     -> ButtonInteraction {
    let st = ButtonInteraction::interact(ui, rect, index, view);
    // The focus ring shows whether or not the window is active.
    let focused = view.frame.focus_visible && st.focused;
    let target = if st.pointer_down || st.key_pressed {
        // The pressed look stays while the pointer is dragged off the button.
        tk.pressed
    } else if st.contains_pointer {
        // Hover is geometric, even while another button is held.
        tk.hover
    } else if focused && !focus_suppressed {
        tk.focused
    } else {
        tk.idle
    };
    let look = ui.animate(button_id(index), target, FADE);
    ui.fill_rect(rect, BUTTON_RADIUS, look.fill);
    ui.stroke_rect(rect, BUTTON_RADIUS, BUTTON_BORDER, look.border);
    ui.text(label, caps_centered(rect, label), look.text);
    st
}

// ------------------------------------------------------------------------------------------------
// Progress
// ------------------------------------------------------------------------------------------------

/// Stable id: the value tween must survive indeterminate phases (the next value animates from
/// the last determinate bar end).
fn progress_id() -> Id {
    Id::new("ubuntu.progress.value")
}

/// The progress bar in `rect` (`PROGRESS_H` tall): a radius-2 track + bar animating to each new
/// value over 300 ms OutCubic; indeterminate = a pill track with a "stretchy capsule" on a 3 s
/// loop.
pub(crate) fn progress(ui: &mut Ui<'_>, rect: Rect, progress: ProgressView, tk: &UbuntuTokens) {
    match progress {
        ProgressView::Determinate { value } => {
            let v = ui.animate(progress_id(), value.clamp(0.0, 1.0), VALUE_ANIM).clamp(0.0, 1.0) as f64;
            ui.fill_rect(rect, PROGRESS_RADIUS, tk.progress_bg);
            if v > 0.0 {
                ui.fill_rect_unsnapped(Rect::from_origin_size(rect.origin(), Size::new(v * rect.width(), rect.height())),
                                       PROGRESS_RADIUS,
                                       tk.progress_fg);
            }
        }
        ProgressView::Indeterminate { restarted_at, .. } => {
            // A running value animation freezes while indeterminate.
            ui.stop_animation::<f32>(progress_id());
            let r = rect.height() / 2.0;
            ui.fill_rect(rect, r, tk.progress_bg);
            let elapsed = (ui.time() - restarted_at).max(0.0);
            let pos = capsule_pos((elapsed.rem_euclid(CYCLE) / CYCLE) as f32) as f64;
            let (w, d) = (rect.width(), rect.height());
            let len = d + STRETCH * (w - d);
            let cx = (d - len / 2.0) + pos * (w - 2.0 * d + len);
            let (left, right) = ((cx - len / 2.0).max(0.0), (cx + len / 2.0).min(w));
            if right > left {
                ui.fill_rect_unsnapped(Rect::new(rect.x0 + left, rect.y0, rect.x0 + right, rect.y1), r, tk.progress_fg);
            }
            ui.request_smooth_frame();
        }
    }
}

/// Capsule travel position 0..1 at normalized cycle time `n`: 0-40 % sweep right, 40-50 % hold,
/// 50-90 % sweep back, 90-100 % hold; each sweep eases with smoothstep.
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

// ------------------------------------------------------------------------------------------------
// Icons: drawn procedurally, a filled disc with a white "i" (information), a white X (error) or a
// dark "!" (warning). Every proportion is relative to the icon size.
// ------------------------------------------------------------------------------------------------

const INFO: Color = rgb(0x2196F3);
const ERROR: Color = rgb(0xD75A4A);
const WARNING: Color = rgb(0xFFC107);
const WARNING_GLYPH: Color = rgb(0x3D3D3D);

/// Draw the view's icon into the square `rect` (logical px): a severity icon, or the custom image.
pub(crate) fn icon(ui: &mut Ui<'_>, view: &DialogView<'_>, rect: Rect) {
    let s = rect.width();
    let c = rect.center();
    let disc = |ui: &mut Ui<'_>, col| ui.circle(c, s / 2.0 - 1.0, col);
    // A vertical stem of width `0.1 s` whose top end is `top` below the centre, `h` long overall
    // (round ends included).
    let stem = |ui: &mut Ui<'_>, top: f64, h: f64, col| {
        let w = s * 0.1;
        ui.line(Point::new(c.x, c.y + top + w / 2.0), Point::new(c.x, c.y + top + h - w / 2.0), w, LineCap::Round, col);
    };
    match view.icon {
        XDialogIcon::None => {}
        XDialogIcon::Custom => {
            if let Some(image) = view.custom_icon {
                ui.image(image, rect);
            }
        }
        XDialogIcon::Information => {
            disc(ui, INFO);
            ui.circle(c - Vec2::new(0.0, s * 0.2), s * 0.07, Color::WHITE);
            stem(ui, -s * 0.05, s * 0.3, Color::WHITE);
        }
        XDialogIcon::Error => {
            disc(ui, ERROR);
            let (arm, width) = (s * 0.18, s * 0.08);
            for d in [Vec2::new(arm, arm), Vec2::new(arm, -arm)] {
                ui.line(c - d, c + d, width, LineCap::Round, Color::WHITE);
            }
        }
        XDialogIcon::Warning => {
            disc(ui, WARNING);
            stem(ui, -s * 0.25, s * 0.28, WARNING_GLYPH);
            ui.circle(c + Vec2::new(0.0, s * 0.18), s * 0.065, WARNING_GLYPH);
        }
    }
}
