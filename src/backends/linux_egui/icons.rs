//! The dialog icons, drawn procedurally with egui painter primitives: a filled disc with a white
//! "i" (information), a white X (error) or a dark "!" (warning). Every proportion is relative to
//! the icon size.

use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};

use crate::backends::egui_core::color::rgb;
use crate::model::XDialogIcon;

const INFO: Color32 = rgb(0x2196F3);
const ERROR: Color32 = rgb(0xD75A4A);
const WARNING: Color32 = rgb(0xFFC107);
const WARNING_GLYPH: Color32 = rgb(0x3D3D3D);

/// Draw `icon` into the square `rect` (logical px).
pub(crate) fn draw_icon(painter: &Painter, icon: &XDialogIcon, rect: Rect) {
    let s = rect.width();
    let c = rect.center();
    let disc = |col| painter.circle_filled(c, s / 2.0 - 1.0, col);
    // A vertical pill of width `0.1 s` whose top edge is `top` below the centre.
    let stem = |top: f32, h: f32, col| {
        let w = s * 0.1;
        painter.rect_filled(Rect::from_min_size(Pos2::new(c.x - w / 2.0, c.y + top), Vec2::new(w, h)), w / 2.0, col);
    };
    match icon {
        XDialogIcon::None => {}
        XDialogIcon::Information => {
            disc(INFO);
            painter.circle_filled(c - Vec2::new(0.0, s * 0.2), s * 0.07, Color32::WHITE);
            stem(-s * 0.05, s * 0.3, Color32::WHITE);
        }
        XDialogIcon::Error => {
            disc(ERROR);
            let (arm, width) = (s * 0.18, s * 0.08);
            for d in [Vec2::new(arm, arm), Vec2::new(arm, -arm)] {
                let (a, b) = (c - d, c + d);
                painter.line_segment([a, b], Stroke::new(width, Color32::WHITE));
                // Round caps.
                painter.circle_filled(a, width / 2.0, Color32::WHITE);
                painter.circle_filled(b, width / 2.0, Color32::WHITE);
            }
        }
        XDialogIcon::Warning => {
            disc(WARNING);
            stem(-s * 0.25, s * 0.28, WARNING_GLYPH);
            painter.circle_filled(c + Vec2::new(0.0, s * 0.18), s * 0.065, WARNING_GLYPH);
        }
    }
}
