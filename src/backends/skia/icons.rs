use tiny_skia::Pixmap;

use super::renderer::{fill_circle, fill_rounded_rect, stroke_line};

use crate::model::XDialogIcon;

/// Render an icon to a pixmap at the given size.
pub fn render_icon(icon: XDialogIcon, size: u32) -> Option<Pixmap> {
    match icon {
        XDialogIcon::None => None,
        XDialogIcon::Information => Some(render_info_icon(size)),
        XDialogIcon::Error => Some(render_error_icon(size)),
        XDialogIcon::Warning => Some(render_warning_icon(size)),
    }
}

/// Blue circle with white "i"
fn render_info_icon(size: u32) -> Pixmap {
    let mut pixmap = Pixmap::new(size, size).unwrap();
    let s = size as f32;
    let center = s / 2.0;
    let radius = s / 2.0 - 1.0;

    // Blue circle background
    fill_circle(&mut pixmap.as_mut(), center, center, radius, (0x21, 0x96, 0xF3));

    // White "i" - dot
    let dot_radius = s * 0.07;
    let dot_y = center - s * 0.2;
    fill_circle(&mut pixmap.as_mut(), center, dot_y, dot_radius, (0xFF, 0xFF, 0xFF));

    // White "i" - body (rounded rect)
    let body_w = s * 0.1;
    let body_h = s * 0.3;
    let body_x = center - body_w / 2.0;
    let body_y = center - s * 0.05;
    fill_rounded_rect(
        &mut pixmap.as_mut(),
        body_x,
        body_y,
        body_w,
        body_h,
        body_w / 2.0,
        (0xFF, 0xFF, 0xFF),
    );

    pixmap
}

/// Red circle with white "X"
fn render_error_icon(size: u32) -> Pixmap {
    let mut pixmap = Pixmap::new(size, size).unwrap();
    let s = size as f32;
    let center = s / 2.0;
    let radius = s / 2.0 - 1.0;

    // Red circle background
    fill_circle(&mut pixmap.as_mut(), center, center, radius, (0xD7, 0x5A, 0x4A));

    // White X
    let arm = s * 0.18;
    let stroke_w = s * 0.08;
    stroke_line(
        &mut pixmap.as_mut(),
        center - arm,
        center - arm,
        center + arm,
        center + arm,
        (0xFF, 0xFF, 0xFF),
        stroke_w,
    );
    stroke_line(
        &mut pixmap.as_mut(),
        center + arm,
        center - arm,
        center - arm,
        center + arm,
        (0xFF, 0xFF, 0xFF),
        stroke_w,
    );

    pixmap
}

/// Yellow/orange circle with dark "!"
fn render_warning_icon(size: u32) -> Pixmap {
    let mut pixmap = Pixmap::new(size, size).unwrap();
    let s = size as f32;
    let center = s / 2.0;
    let radius = s / 2.0 - 1.0;

    // Yellow circle background
    fill_circle(&mut pixmap.as_mut(), center, center, radius, (0xFF, 0xC1, 0x07));

    // Dark "!" - body (rounded rect)
    let body_w = s * 0.1;
    let body_h = s * 0.28;
    let body_x = center - body_w / 2.0;
    let body_y = center - s * 0.25;
    fill_rounded_rect(
        &mut pixmap.as_mut(),
        body_x,
        body_y,
        body_w,
        body_h,
        body_w / 2.0,
        (0x3D, 0x3D, 0x3D),
    );

    // Dark "!" - dot
    let dot_radius = s * 0.065;
    let dot_y = center + s * 0.18;
    fill_circle(&mut pixmap.as_mut(), center, dot_y, dot_radius, (0x3D, 0x3D, 0x3D));

    pixmap
}
