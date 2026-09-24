//! Painting helper library for theme widgets. Stateless functions on an
//! `egui::Painter`: pixel snapping, rounded rects with circular or skia-style quadratic corners,
//! centred strokes, per-edge border colours. Signatures are frozen (additions only); bodies may
//! be refined. All geometry is in logical px; pass `ppp` (`ui.ctx().pixels_per_point()`) where
//! snapping/tessellation matters.

use egui::epaint::PathShape;
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};

/// Round a logical coordinate to the nearest physical pixel boundary.
pub(crate) fn snap_len(v: f32, ppp: f32) -> f32 {
    if ppp <= 0.0 {
        return v;
    }
    (v * ppp).round() / ppp
}

/// Snap a point to the physical pixel grid.
pub(crate) fn snap_pos(p: Pos2, ppp: f32) -> Pos2 {
    Pos2::new(snap_len(p.x, ppp), snap_len(p.y, ppp))
}

/// Snap all four edges of `rect` to physical pixel boundaries (use for 1px borders/separators).
pub(crate) fn snap(rect: Rect, ppp: f32) -> Rect {
    Rect::from_min_max(snap_pos(rect.min, ppp), snap_pos(rect.max, ppp))
}

/// How rounded-rect corners are built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CornerStyle {
    /// True circular arcs (egui/GTK/WinUI).
    Circular,
    /// Quadratic Bézier corners with the control point at the rect corner (the former skia
    /// backend's `rounded_rect_path`, which looks slightly "squarer").
    Quadratic,
}

/// Closed outline (clockwise from the top edge) of a rounded rect. `radius` is clamped to
/// `min(w, h) / 2`. `ppp` controls the corner tessellation density.
pub(crate) fn rounded_rect_path(rect: Rect, radius: f32, style: CornerStyle, ppp: f32) -> Vec<Pos2> {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
    if r <= 0.0 {
        return vec![rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    }
    let segs = ((r * ppp.max(1.0)).ceil() as usize).clamp(4, 32);
    // (corner point, start of corner, end of corner) clockwise, starting top-right.
    let corners = [(rect.right_top(), Pos2::new(rect.right() - r, rect.top()), Pos2::new(rect.right(), rect.top() + r)),
                   (rect.right_bottom(), Pos2::new(rect.right(), rect.bottom() - r), Pos2::new(rect.right() - r, rect.bottom())),
                   (rect.left_bottom(), Pos2::new(rect.left() + r, rect.bottom()), Pos2::new(rect.left(), rect.bottom() - r)),
                   (rect.left_top(), Pos2::new(rect.left(), rect.top() + r), Pos2::new(rect.left() + r, rect.top()))];
    let mut pts = Vec::with_capacity(4 * (segs + 1));
    for (corner, a, b) in corners {
        for i in 0..=segs {
            let t = i as f32 / segs as f32;
            let p = match style {
                CornerStyle::Quadratic => {
                    let u = 1.0 - t;
                    Pos2::new(u * u * a.x + 2.0 * u * t * corner.x + t * t * b.x, u * u * a.y + 2.0 * u * t * corner.y + t * t * b.y)
                }
                CornerStyle::Circular => {
                    // Arc centre is the corner moved inward by r on both axes.
                    let center = Pos2::new(a.x + (b.x - corner.x), a.y + (b.y - corner.y));
                    let va = a - center;
                    let vb = b - center;
                    let a0 = va.y.atan2(va.x);
                    let mut a1 = vb.y.atan2(vb.x);
                    if a1 < a0 {
                        a1 += std::f32::consts::TAU;
                    }
                    let ang = a0 + (a1 - a0) * t;
                    center + Vec2::new(ang.cos(), ang.sin()) * r
                }
            };
            pts.push(p);
        }
    }
    pts
}

/// Fill a rounded rect (anti-aliased).
pub(crate) fn fill_rounded_rect(painter: &Painter, rect: Rect, radius: f32, style: CornerStyle, fill: Color32, ppp: f32) {
    if fill == Color32::TRANSPARENT {
        return;
    }
    if style == CornerStyle::Circular {
        painter.rect_filled(rect, radius, fill);
        return;
    }
    painter.add(Shape::Path(PathShape::convex_polygon(rounded_rect_path(rect, radius, style, ppp), fill, Stroke::NONE)));
}

/// Stroke a rounded rect outline of `width`, CENTRED on the rect edge (half falls outside, as in
/// the skia backend).
pub(crate) fn stroke_rounded_rect(painter: &Painter, rect: Rect, radius: f32, style: CornerStyle, width: f32, color: Color32, ppp: f32) {
    if width <= 0.0 || color == Color32::TRANSPARENT {
        return;
    }
    painter.add(Shape::Path(PathShape::closed_line(rounded_rect_path(rect, radius, style, ppp), Stroke::new(width, color))));
}

/// Per-edge border colours (Fluent elevation borders).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EdgeColors {
    pub top: Color32,
    pub right: Color32,
    pub bottom: Color32,
    pub left: Color32,
}
