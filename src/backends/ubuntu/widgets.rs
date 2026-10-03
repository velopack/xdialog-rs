//! The Ubuntu theme's widgets: the outlined button, the progress bar, the look of the body
//! scroll bar and the icons.

use std::rc::Rc;

use super::tokens::*;
use crate::backends::draw::color::rgb;
use crate::backends::draw::{Color, LineCap, Point, Rect, Size, Vec2};
use crate::backends::gui::anim::{Easing, Lerp, Transition};
use crate::backends::gui::text::TextBlock;
use crate::backends::gui::theme::{button_id, ButtonInteraction, DialogView, ProgressView};
use crate::backends::gui::ui::{caps_centered, Id, ScrollBarSpec, Ui};
use crate::model::XDialogIcon;

const FADE: Transition = Transition::linear(0.15);
const VALUE_ANIM: Transition = Transition::new(0.3, Easing::OutCubic);

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

/// Natural width of a button with `label` (no minimum).
pub(crate) fn button_width(label: &TextBlock) -> f64 {
    label.size.width + 2.0 * BUTTON_PAD_X
}

/// Outlined rounded button in `rect`: the border centred on the edge, the label centred
/// (capitals centred vertically). State priority `Pressed > Hovered > Focused > Idle`; every
/// change fades all three colours from the displayed value. The focus border is hidden while
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

/// The progress bar in `rect`: a rounded track + bar animating to each new value;
/// indeterminate = a pill track with the shared "stretchy capsule" (`Ui::indeterminate_capsule`).
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
            ui.fill_rect(rect, rect.height() / 2.0, tk.progress_bg);
            ui.indeterminate_capsule(rect, restarted_at, tk.progress_fg);
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Scroll bar
// ------------------------------------------------------------------------------------------------

/// GTK's overlay scroll bar (Yaru), in `UbuntuTokens::scroll_thumb`.
pub(crate) const SCROLL_BAR: ScrollBarSpec = ScrollBarSpec { thin: 3.0,
                                                             wide: 8.0,
                                                             inset: 3.0,
                                                             end_inset: 3.0,
                                                             min_thumb: 40.0,
                                                             fade: FADE,
                                                             max_radius: None };

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
