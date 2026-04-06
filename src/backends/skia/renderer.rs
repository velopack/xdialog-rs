use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, PixmapMut, Stroke, Transform};

/// Fill a rectangle with a solid color.
pub fn fill_rect(pixmap: &mut PixmapMut, x: f32, y: f32, w: f32, h: f32, color: (u8, u8, u8)) {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(color.0, color.1, color.2, 255));
    paint.anti_alias = false;

    if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) {
        let path = PathBuilder::from_rect(rect);
        pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

/// Build a rounded rectangle path.
fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);

    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Fill a rounded rectangle.
pub fn fill_rounded_rect(
    pixmap: &mut PixmapMut,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    color: (u8, u8, u8),
) {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(color.0, color.1, color.2, 255));
    paint.anti_alias = true;

    if let Some(path) = rounded_rect_path(x, y, w, h, radius) {
        pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

/// Stroke a rounded rectangle outline.
#[allow(clippy::too_many_arguments)]
pub fn stroke_rounded_rect(
    pixmap: &mut PixmapMut,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    color: (u8, u8, u8),
    stroke_width: f32,
) {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(color.0, color.1, color.2, 255));
    paint.anti_alias = true;

    let stroke = Stroke {
        width: stroke_width,
        ..Stroke::default()
    };

    if let Some(path) = rounded_rect_path(x, y, w, h, radius) {
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

/// Composite a source pixmap onto a destination pixmap at the given position.
pub fn composite_pixmap(dst: &mut PixmapMut, src: &Pixmap, x: i32, y: i32) {
    let src_w = src.width() as i32;
    let src_h = src.height() as i32;
    let dst_w = dst.width() as i32;
    let dst_h = dst.height() as i32;
    let src_data = src.data();
    let dst_data = dst.data_mut();

    for sy in 0..src_h {
        let dy = y + sy;
        if dy < 0 || dy >= dst_h {
            continue;
        }
        for sx in 0..src_w {
            let dx = x + sx;
            if dx < 0 || dx >= dst_w {
                continue;
            }
            let si = (sy as usize * src_w as usize + sx as usize) * 4;
            let di = (dy as usize * dst_w as usize + dx as usize) * 4;
            let sa = src_data[si + 3] as f32 / 255.0;
            if sa <= 0.0 {
                continue;
            }
            if sa >= 1.0 {
                dst_data[di] = src_data[si];
                dst_data[di + 1] = src_data[si + 1];
                dst_data[di + 2] = src_data[si + 2];
                dst_data[di + 3] = 255;
            } else {
                let inv_a = 1.0 - sa;
                dst_data[di] = (src_data[si] as f32 * sa + dst_data[di] as f32 * inv_a) as u8;
                dst_data[di + 1] =
                    (src_data[si + 1] as f32 * sa + dst_data[di + 1] as f32 * inv_a) as u8;
                dst_data[di + 2] =
                    (src_data[si + 2] as f32 * sa + dst_data[di + 2] as f32 * inv_a) as u8;
                dst_data[di + 3] =
                    (255.0f32.min(dst_data[di + 3] as f32 + src_data[si + 3] as f32)) as u8;
            }
        }
    }
}

/// Fill a circle.
pub fn fill_circle(
    pixmap: &mut PixmapMut,
    cx: f32,
    cy: f32,
    radius: f32,
    color: (u8, u8, u8),
) {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(color.0, color.1, color.2, 255));
    paint.anti_alias = true;

    let mut pb = PathBuilder::new();
    // Approximate circle with 4 cubic bezier curves
    let k = 0.552_284_8; // magic number for circle approximation
    let r = radius;
    pb.move_to(cx, cy - r);
    pb.cubic_to(cx + r * k, cy - r, cx + r, cy - r * k, cx + r, cy);
    pb.cubic_to(cx + r, cy + r * k, cx + r * k, cy + r, cx, cy + r);
    pb.cubic_to(cx - r * k, cy + r, cx - r, cy + r * k, cx - r, cy);
    pb.cubic_to(cx - r, cy - r * k, cx - r * k, cy - r, cx, cy - r);
    pb.close();

    if let Some(path) = pb.finish() {
        pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

/// Stroke a line.
pub fn stroke_line(
    pixmap: &mut PixmapMut,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    color: (u8, u8, u8),
    width: f32,
) {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(color.0, color.1, color.2, 255));
    paint.anti_alias = true;

    let stroke = Stroke {
        width,
        line_cap: tiny_skia::LineCap::Round,
        ..Stroke::default()
    };

    let mut pb = PathBuilder::new();
    pb.move_to(x1, y1);
    pb.line_to(x2, y2);

    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}
