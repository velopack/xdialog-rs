use tiny_skia::PixmapMut;

use super::renderer::{fill_circle, fill_rounded_rect, stroke_line};

use crate::model::XDialogIcon;

/// Draw an icon directly onto the target pixmap at the given position and size.
pub fn draw_icon(pixmap: &mut PixmapMut, icon: &XDialogIcon, x: f32, y: f32, size: f32) {
    match icon {
        XDialogIcon::None => {}
        XDialogIcon::Information => draw_info_icon(pixmap, x, y, size),
        XDialogIcon::Error => draw_error_icon(pixmap, x, y, size),
        XDialogIcon::Warning => draw_warning_icon(pixmap, x, y, size),
    }
}

/// Blue circle with white "i"
fn draw_info_icon(pixmap: &mut PixmapMut, x: f32, y: f32, s: f32) {
    let cx = x + s / 2.0;
    let cy = y + s / 2.0;
    let radius = s / 2.0 - 1.0;

    fill_circle(pixmap, cx, cy, radius, (0x21, 0x96, 0xF3));

    // Dot
    let dot_radius = s * 0.07;
    let dot_y = cy - s * 0.2;
    fill_circle(pixmap, cx, dot_y, dot_radius, (0xFF, 0xFF, 0xFF));

    // Body
    let body_w = s * 0.1;
    let body_h = s * 0.3;
    let body_x = cx - body_w / 2.0;
    let body_y = cy - s * 0.05;
    fill_rounded_rect(pixmap, body_x, body_y, body_w, body_h, body_w / 2.0, (0xFF, 0xFF, 0xFF));
}

/// Red circle with white "X"
fn draw_error_icon(pixmap: &mut PixmapMut, x: f32, y: f32, s: f32) {
    let cx = x + s / 2.0;
    let cy = y + s / 2.0;
    let radius = s / 2.0 - 1.0;

    fill_circle(pixmap, cx, cy, radius, (0xD7, 0x5A, 0x4A));

    let arm = s * 0.18;
    let stroke_w = s * 0.08;
    stroke_line(pixmap, cx - arm, cy - arm, cx + arm, cy + arm, (0xFF, 0xFF, 0xFF), stroke_w);
    stroke_line(pixmap, cx + arm, cy - arm, cx - arm, cy + arm, (0xFF, 0xFF, 0xFF), stroke_w);
}

/// Yellow/orange circle with dark "!"
fn draw_warning_icon(pixmap: &mut PixmapMut, x: f32, y: f32, s: f32) {
    let cx = x + s / 2.0;
    let cy = y + s / 2.0;
    let radius = s / 2.0 - 1.0;

    fill_circle(pixmap, cx, cy, radius, (0xFF, 0xC1, 0x07));

    // Body
    let body_w = s * 0.1;
    let body_h = s * 0.28;
    let body_x = cx - body_w / 2.0;
    let body_y = cy - s * 0.25;
    fill_rounded_rect(pixmap, body_x, body_y, body_w, body_h, body_w / 2.0, (0x3D, 0x3D, 0x3D));

    // Dot
    let dot_radius = s * 0.065;
    let dot_y = cy + s * 0.18;
    fill_circle(pixmap, cx, dot_y, dot_radius, (0x3D, 0x3D, 0x3D));
}
