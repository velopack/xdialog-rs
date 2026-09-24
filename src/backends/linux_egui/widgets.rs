//! The Linux theme's widgets: the outlined button and the progress bar (icons are in `icons.rs`).

use egui::{Color32, Id, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, Widget};

use super::tokens::*;
use crate::backends::egui_core::anim::{self, Easing, Lerp, Transition};
use crate::backends::egui_core::text::TextBlock;
use crate::backends::egui_core::theme::{ButtonInteraction, DialogView, ProgressView};

/// Button colour fade: 150 ms linear.
const FADE: Transition = Transition::linear(0.15);
/// Progress value animation: 300 ms OutCubic.
const VALUE_ANIM: Transition = Transition::new(0.3, Easing::OutCubic);
/// Indeterminate capsule cycle (s) and its length as a fraction of the free track.
const CYCLE: f64 = 3.0;
const STRETCH: f32 = 0.45;

// ------------------------------------------------------------------------------------------------
// Button
// ------------------------------------------------------------------------------------------------

impl Lerp for ButtonLook {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        ButtonLook { border: Color32::lerp(&a.border, &b.border, t),
                     fill: Color32::lerp(&a.fill, &b.fill, t),
                     text: Color32::lerp(&a.text, &b.text, t) }
    }
}

/// Outlined rounded button: radius 6, a 2 px border centred on the edge, the label centred.
/// State priority `Pressed > Hovered > Focused > Idle`; every change fades all three colours
/// linearly over 150 ms from the displayed value.
pub(crate) struct Button<'a> {
    pub index: usize,
    pub label: &'a TextBlock,
    pub view: &'a DialogView<'a>,
    pub tk: &'a LinuxTokens,
    /// The focus border is hidden while the pointer is over any button (see `focus_suppressed`).
    pub focus_suppressed: bool,
}

impl Button<'_> {
    /// Allocate, interact and paint; returns the interaction so the dialog can report pointer
    /// activation.
    pub(crate) fn show(self, ui: &mut Ui) -> ButtonInteraction {
        // Natural label width + 2 x 24 (no minimum).
        let (rect, _) = ui.allocate_exact_size(Vec2::new(self.label.size.x + 2.0 * BUTTON_PAD_X, BUTTON_H), Sense::hover());
        let st = ButtonInteraction::interact(ui, rect, self.index, self.view);
        let tk = self.tk;
        // The focus ring shows whether or not the window is active. `Response::has_focus` (and so
        // `st.focus_visible`) is false while the window is inactive; egui's focus memory keeps the
        // focused widget, so read that instead (not in the measure pass, which stays idle).
        let focused = !self.view.frame.sizing && self.view.frame.focus_visible && ui.ctx().memory(|m| m.has_focus(st.response.id));
        let target = if st.pointer_down || st.key_pressed {
            // The pressed look stays while the pointer is dragged off the button.
            tk.pressed
        } else if st.contains_pointer && ui.input(|i| i.pointer.latest_pos().is_some()) {
            // Hover is geometric, even while another button is held. egui still reports
            // `contains_pointer` on the pass that delivers `PointerGone` (it clears the interact
            // position a pass later), and nothing may request that pass: check the pointer.
            tk.hover
        } else if focused && !self.focus_suppressed {
            tk.focused
        } else {
            tk.idle
        };
        let look = anim::animate(ui.ctx(), st.response.id, target, FADE);
        if ui.is_rect_visible(rect.expand(BUTTON_BORDER)) {
            let painter = ui.painter();
            painter.rect_filled(rect, BUTTON_RADIUS, look.fill);
            painter.rect_stroke(rect, BUTTON_RADIUS, Stroke::new(BUTTON_BORDER, look.border), StrokeKind::Middle);
            self.label.paint(painter, rect.center() - self.label.size / 2.0, look.text);
        }
        st
    }
}

// ------------------------------------------------------------------------------------------------
// Progress
// ------------------------------------------------------------------------------------------------

/// The progress bar, `width` x 6: a radius-2 track + bar animating to each new value over
/// 300 ms OutCubic; indeterminate = a pill track with a "stretchy capsule" on a 3 s loop.
pub(crate) struct ProgressBar<'a> {
    pub progress: ProgressView,
    pub width: f32,
    pub tk: &'a LinuxTokens,
}

/// Stable id: the value tween must survive indeterminate phases (the next value animates from
/// the last determinate bar end).
fn progress_id() -> Id {
    Id::new("linux.progress.value")
}

impl Widget for ProgressBar<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::new(self.width, PROGRESS_H), Sense::hover());
        let ctx = ui.ctx().clone();
        let painter = ui.painter();
        let tk = self.tk;
        match self.progress {
            ProgressView::Determinate { value } => {
                let v = anim::animate(&ctx, progress_id(), value.clamp(0.0, 1.0), VALUE_ANIM).clamp(0.0, 1.0);
                painter.rect_filled(rect, PROGRESS_RADIUS, tk.progress_bg);
                if v > 0.0 {
                    let bar = Rect::from_min_size(rect.min, Vec2::new(v * rect.width(), rect.height()));
                    painter.rect_filled(bar, PROGRESS_RADIUS, tk.progress_fg);
                }
            }
            ProgressView::Indeterminate { restarted_at, .. } => {
                // A running value animation freezes while indeterminate.
                anim::stop::<f32>(&ctx, progress_id());
                let r = rect.height() / 2.0;
                painter.rect_filled(rect, r, tk.progress_bg);
                let elapsed = (ui.input(|i| i.time) - restarted_at).max(0.0);
                let pos = capsule_pos((elapsed.rem_euclid(CYCLE) / CYCLE) as f32);
                let (w, d) = (rect.width(), rect.height());
                let len = d + STRETCH * (w - d);
                let cx = (d - len / 2.0) + pos * (w - 2.0 * d + len);
                let (left, right) = ((cx - len / 2.0).max(0.0), (cx + len / 2.0).min(w));
                if right > left {
                    let cap = Rect::from_x_y_ranges(rect.left() + left..=rect.left() + right, rect.y_range());
                    painter.rect_filled(cap, r, tk.progress_fg);
                }
                ctx.request_repaint();
            }
        }
        response
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
