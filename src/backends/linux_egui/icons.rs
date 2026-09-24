//! 1:1 port of the skia backend's procedural icons (`icons.rs` / `icon.rs`), drawn
//! with egui painter primitives.
//!
//! skia rasterized each icon into a `round(48 * s)` px tile composited at the rounded physical
//! position, and every proportion (including the circle's `- 1`) is relative to that tile size in
//! PHYSICAL px. This port keeps that: geometry is computed in physical px, then divided by `ppp`.

use egui::epaint::PathShape;
use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, Widget};

use crate::backends::egui_core::color::rgb;
use crate::backends::egui_core::paint_util::{self, CornerStyle};
use crate::model::XDialogIcon;

const INFO: Color32 = rgb(0x2196F3);
const ERROR: Color32 = rgb(0xD75A4A);
const WARNING: Color32 = rgb(0xFFC107);
const WARNING_GLYPH: Color32 = rgb(0x3D3D3D);
/// epaint anti-aliases with a 1 px linear ramp across each edge, which over-covers the outside
/// pixels of slanted edges compared to tiny-skia's supersampled coverage (the error X came out
/// ~6 % heavier, the dots ~3 %; the large circles match as they are). Insetting the outlines by
/// these physical-px amounts matches the captured coverage (measured on the static captures).
const DOT_AA_INSET: f32 = 0.04;
const CAPSULE_AA_INSET: f32 = 0.08;

/// The dialog icon at a fixed logical rect (top-left at `(16, 16)`, 48x48).
pub(crate) struct SkiaIcon<'a> {
    pub icon: &'a XDialogIcon,
    /// Logical rect of the icon slot.
    pub rect: Rect,
}

impl Widget for SkiaIcon<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let response = ui.allocate_rect(self.rect, Sense::hover());
        if ui.is_rect_visible(self.rect) {
            let ppp = ui.ctx().pixels_per_point();
            // Tile: rounded physical origin, rounded physical size (icon.rs:67-87).
            let origin = Pos2::new((self.rect.min.x * ppp).round(), (self.rect.min.y * ppp).round());
            let size = (self.rect.width() * ppp).round();
            if size > 0.0 {
                draw_icon(ui.painter(), self.icon, origin, size, ppp);
            }
        }
        response
    }
}

/// Physical -> logical helper.
struct Px {
    ppp: f32,
}

impl Px {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(x / self.ppp, y / self.ppp)
    }
    fn l(&self, v: f32) -> f32 {
        v / self.ppp
    }
}

/// Draw `icon` into the physical tile at `origin` (physical px) of `s` x `s` physical px.
pub(crate) fn draw_icon(painter: &Painter, icon: &XDialogIcon, origin: Pos2, s: f32, ppp: f32) {
    let px = Px { ppp };
    let cx = origin.x + s / 2.0;
    let cy = origin.y + s / 2.0;
    let radius = s / 2.0 - 1.0;
    match icon {
        XDialogIcon::None => {}
        XDialogIcon::Information => {
            circle(painter, &px, cx, cy, radius, 0.0, INFO);
            // Dot.
            circle(painter, &px, cx, cy - s * 0.2, s * 0.07, DOT_AA_INSET, Color32::WHITE);
            // Stem: rounded rect (quadratic corners, radius w/2).
            let (w, h) = (s * 0.1, s * 0.3);
            stem(painter, &px, cx - w / 2.0, cy - s * 0.05, w, h, Color32::WHITE);
        }
        XDialogIcon::Error => {
            circle(painter, &px, cx, cy, radius, 0.0, ERROR);
            let arm = s * 0.18;
            let width = s * 0.08;
            capsule(painter, &px, (cx - arm, cy - arm), (cx + arm, cy + arm), width, Color32::WHITE);
            capsule(painter, &px, (cx + arm, cy - arm), (cx - arm, cy + arm), width, Color32::WHITE);
        }
        XDialogIcon::Warning => {
            circle(painter, &px, cx, cy, radius, 0.0, WARNING);
            let (w, h) = (s * 0.1, s * 0.28);
            stem(painter, &px, cx - w / 2.0, cy - s * 0.25, w, h, WARNING_GLYPH);
            circle(painter, &px, cx, cy + s * 0.18, s * 0.065, DOT_AA_INSET, WARNING_GLYPH);
        }
    }
}

/// skia `fill_circle`: four cubic Béziers (k = 0.5522848), filled anti-aliased. Drawn as a finely
/// flattened convex polygon: `painter.circle_filled` uses epaint's pre-rasterized discs for small
/// radii, which are snapped and softer (the icon dots came out ~0.4 px off and one row wider).
fn circle(painter: &Painter, px: &Px, cx: f32, cy: f32, r: f32, inset: f32, color: Color32) {
    const K: f32 = 0.552_284_8;
    let (c, r) = (px.p(cx, cy), px.l(r - inset));
    // Per quadrant (clockwise on screen, from the top): start, control 1, control 2, end.
    let quads = [[(0.0, -1.0), (K, -1.0), (1.0, -K), (1.0, 0.0)],
                 [(1.0, 0.0), (1.0, K), (K, 1.0), (0.0, 1.0)],
                 [(0.0, 1.0), (-K, 1.0), (-1.0, K), (-1.0, 0.0)],
                 [(-1.0, 0.0), (-1.0, -K), (-K, -1.0), (0.0, -1.0)]];
    let segs = ((r * px.ppp).ceil() as usize * 2).clamp(8, 64);
    let mut pts = Vec::with_capacity(4 * segs);
    for q in quads {
        for i in 0..segs {
            let t = i as f32 / segs as f32;
            let u = 1.0 - t;
            let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            let x = w0 * q[0].0 + w1 * q[1].0 + w2 * q[2].0 + w3 * q[3].0;
            let y = w0 * q[0].1 + w1 * q[1].1 + w2 * q[2].1 + w3 * q[3].1;
            pts.push(c + Vec2::new(x, y) * r);
        }
    }
    painter.add(Shape::Path(PathShape::convex_polygon(pts, color, Stroke::NONE)));
}

/// skia `fill_rounded_rect(x, y, w, h, w / 2)`: quadratic corners.
fn stem(painter: &Painter, px: &Px, x: f32, y: f32, w: f32, h: f32, color: Color32) {
    let rect = Rect::from_min_size(px.p(x, y), Vec2::new(px.l(w), px.l(h)));
    paint_util::fill_rounded_rect(painter, rect, px.l(w / 2.0), CornerStyle::Quadratic, color, px.ppp);
}

/// A round-capped line of `width` (tiny-skia `LineCap::Round`) as ONE convex outline, so the caps
/// don't double-blend their anti-aliased seams with the line body.
fn capsule(painter: &Painter, px: &Px, a: (f32, f32), b: (f32, f32), width: f32, color: Color32) {
    let (a, b) = (px.p(a.0, a.1), px.p(b.0, b.1));
    let r = px.l(width / 2.0 - CAPSULE_AA_INSET);
    let dir = (b - a).normalized();
    let normal = Vec2::new(-dir.y, dir.x);
    let base = normal.y.atan2(normal.x);
    let segs = 16;
    let mut pts = Vec::with_capacity(2 * (segs + 1));
    // Cap around b: from +normal through +dir to -normal; then around a back to +normal.
    for (center, start) in [(b, base), (a, base + std::f32::consts::PI)] {
        for i in 0..=segs {
            let ang = start - std::f32::consts::PI * i as f32 / segs as f32;
            pts.push(center + Vec2::new(ang.cos(), ang.sin()) * r);
        }
    }
    // egui feathers convex polygons outward only for clockwise (screen, y down) outlines.
    pts.reverse();
    painter.add(Shape::Path(PathShape::convex_polygon(pts, color, Stroke::NONE)));
}
