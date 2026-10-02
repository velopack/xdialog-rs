//! Drawing on vello_cpu.
//!
//! A [`Painter`] (one per surface) owns the vello_cpu render context, its resources (glyph
//! caches), the target pixmap and the text and image caches, all reused across frames; a
//! [`Canvas`] is one frame being drawn with it.
//!
//! Text layouts are rasterized once per layout and scale ([`TextRaster`]) and composited as
//! images: glyph outlines, hinting and colour glyphs (COLR/CBDT emoji) are far too expensive to
//! redo every frame. Monochrome glyphs are drawn white and tinted with the draw colour when
//! composited, so colour fades reuse the raster; colour glyphs keep their own colours.

use std::collections::HashMap;
use std::sync::Arc;

use vello_common::paint::{Tint, TintMode};
use vello_cpu::color::PremulRgba8;
use vello_cpu::kurbo::{self as vk, Affine, Shape as _};
use vello_cpu::peniko::{ImageQuality, ImageSampler};
use vello_cpu::{Glyph, ImageSource, Pixmap, RenderContext, Resources};

use super::text::Shaped;
use crate::backends::draw::{list, Color, Frame, Image, LineCap, Point, Rect, TextLayout};

/// Path flattening tolerance, physical px.
const TOLERANCE: f64 = 0.1;

/// A shaped layout rasterized at one scale: monochrome glyphs (white, tinted when composited) and
/// colour glyphs, each in a pixmap whose top-left is `offset` physical px from the layout origin.
struct TextRaster {
    offset: (f64, f64),
    mono: Option<Arc<Pixmap>>,
    color: Option<Arc<Pixmap>>,
    /// Drawn in the current frame (unused rasters are dropped after it).
    used: bool,
}

impl TextRaster {
    fn new(shaped: &Shaped, scale: f64, resources: &mut Resources) -> Self {
        // Glyphs may reach outside the layout box (accents, descenders, emoji): pad by a font size.
        let pad = (shaped.max_font_size as f64 * scale).ceil();
        let size = shaped.size;
        // Lines aligned to the far end of a wide box (right-to-left paragraphs) start past the
        // measured width: cover every glyph origin (the pad covers the last glyph's advance).
        let right = shaped.runs.iter().flat_map(|r| &r.glyphs).map(|g| g.x as f64).fold(size.width, f64::max);
        let w = (right * scale + 2.0 * pad).ceil().clamp(1.0, u16::MAX as f64) as u16;
        let h = (size.height * scale + 2.0 * pad).ceil().clamp(1.0, u16::MAX as f64) as u16;
        let mut layer = |color: bool| {
            let runs: Vec<_> = shaped.runs.iter().filter(|r| r.face.color == color).collect();
            if runs.is_empty() {
                return None;
            }
            let mut ctx = RenderContext::new(w, h);
            ctx.set_paint(vello_cpu::peniko::Color::WHITE);
            for run in runs {
                let glyphs =
                    run.glyphs
                       .iter()
                       .map(|g| Glyph { id: g.id, x: (pad + g.x as f64 * scale) as f32, y: (pad + (g.y as f64 * scale).round()) as f32 });
                ctx.glyph_run(resources, &run.face.data)
                   .font_size(run.font_size * scale as f32)
                   .normalized_coords(&run.face.coords)
                   .fill_glyphs(glyphs);
            }
            ctx.flush();
            let mut pixmap = Pixmap::new(w, h);
            ctx.render(&mut pixmap, resources);
            Some(Arc::new(pixmap))
        };
        TextRaster { offset: (-pad, -pad), mono: layer(false), color: layer(true), used: true }
    }
}

/// A premultiplied image uploaded as a vello pixmap.
struct CachedImage {
    pixmap: Arc<Pixmap>,
    used: bool,
}

impl CachedImage {
    fn new(image: &Image, w: u16, h: u16) -> Self {
        let data = image.rgba().chunks_exact(4).map(|p| PremulRgba8 { r: p[0], g: p[1], b: p[2], a: p[3] }).collect();
        CachedImage { pixmap: Arc::new(Pixmap::from_parts(data, w, h)), used: true }
    }
}

/// The vello_cpu state of one surface, reused across frames (see the module docs).
pub(crate) struct Painter {
    ctx: RenderContext,
    resources: Resources,
    pixmap: Pixmap,
    /// By (layout id, scale bits).
    texts: HashMap<(u64, u64), TextRaster>,
    /// By image id.
    images: HashMap<u64, CachedImage>,
}

impl Painter {
    pub(crate) fn new() -> Self {
        Painter { ctx: RenderContext::new(1, 1),
                  resources: Resources::new(),
                  pixmap: Pixmap::new(1, 1),
                  texts: HashMap::new(),
                  images: HashMap::new() }
    }

    /// Draw `frame` (both sides of `size_px` non-zero) into [`Painter::pixmap`].
    pub(crate) fn draw(&mut self, frame: &Frame<'_>) {
        let (w, h) = (frame.size_px[0].min(u16::MAX as u32) as u16, frame.size_px[1].min(u16::MAX as u32) as u16);
        if (self.ctx.width(), self.ctx.height()) != (w, h) {
            self.ctx = RenderContext::new(w, h);
            self.pixmap = Pixmap::new(w, h);
        } else {
            self.ctx.reset();
        }
        let mut canvas = Canvas { root: Affine::scale(frame.ppp), ppp: frame.ppp, clips: 0, p: self };
        list::replay(&mut canvas, frame);
        while canvas.clips > 0 {
            crate::backends::draw::Canvas::pop_clip(&mut canvas);
        }
        self.ctx.flush();
        self.ctx.render(&mut self.pixmap, &mut self.resources);
        self.texts.retain(|_, t| std::mem::take(&mut t.used));
        self.images.retain(|_, i| std::mem::take(&mut i.used));
    }

    /// The last frame (premultiplied RGBA8; opaque, as the clear colour is).
    pub(crate) fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }
}

/// A frame being drawn with a [`Painter`], in logical px (the root transform scales by ppp).
pub(crate) struct Canvas<'a> {
    p: &'a mut Painter,
    root: Affine,
    ppp: f64,
    /// Clip layers open.
    clips: usize,
}

fn vcolor(c: Color) -> vello_cpu::peniko::Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    vello_cpu::peniko::Color::from_rgba8(r, g, b, a)
}

fn vpoint(p: Point) -> vk::Point {
    vk::Point::new(p.x, p.y)
}

fn vrect(r: Rect) -> vk::Rect {
    vk::Rect::new(r.x0, r.y0, r.x1, r.y1)
}

impl Canvas<'_> {
    /// The context set up for drawing geometry in logical px with a solid colour.
    fn solid(&mut self, color: Color) -> &mut RenderContext {
        let ctx = &mut self.p.ctx;
        ctx.set_transform(self.root);
        ctx.set_paint(vcolor(color));
        ctx
    }

    /// Flattening tolerance in logical px.
    fn tolerance(&self) -> f64 {
        TOLERANCE / self.ppp
    }
}

impl crate::backends::draw::Canvas for Canvas<'_> {
    fn clear(&mut self, color: Color) {
        let ctx = &mut self.p.ctx;
        let full = vk::Rect::new(0.0, 0.0, ctx.width() as f64, ctx.height() as f64);
        ctx.set_transform(Affine::IDENTITY);
        ctx.set_paint(vcolor(color));
        ctx.fill_rect(&full);
    }

    fn fill_rect(&mut self, rect: Rect, radius: f64, color: Color) {
        let tolerance = self.tolerance();
        let ctx = self.solid(color);
        if radius > 0.0 {
            ctx.fill_path(&vk::RoundedRect::from_rect(vrect(rect), radius).to_path(tolerance));
        } else {
            ctx.fill_rect(&vrect(rect));
        }
    }

    fn stroke_rect(&mut self, rect: Rect, radius: f64, width: f64, color: Color) {
        let tolerance = self.tolerance();
        let ctx = self.solid(color);
        ctx.set_stroke(vk::Stroke::new(width));
        ctx.stroke_path(&vk::RoundedRect::from_rect(vrect(rect), radius).to_path(tolerance));
    }

    fn fill_circle(&mut self, center: Point, radius: f64, color: Color) {
        let tolerance = self.tolerance();
        self.solid(color).fill_path(&vk::Circle::new(vpoint(center), radius).to_path(tolerance));
    }

    fn line(&mut self, from: Point, to: Point, width: f64, cap: LineCap, color: Color) {
        let tolerance = self.tolerance();
        let cap = match cap {
            LineCap::Butt => vk::Cap::Butt,
            LineCap::Round => vk::Cap::Round,
        };
        let ctx = self.solid(color);
        ctx.set_stroke(vk::Stroke::new(width).with_caps(cap));
        ctx.stroke_path(&vk::Line::new(vpoint(from), vpoint(to)).to_path(tolerance));
    }

    fn fill_rect_gradient(&mut self, rect: Rect, radius: f64, top: Color, bottom: Color) {
        use vello_cpu::peniko::{ColorStop, Gradient};
        let tolerance = self.tolerance();
        let gradient = Gradient::new_linear(vk::Point::new(rect.x0, rect.y0), vk::Point::new(rect.x0, rect.y1))
            .with_stops([ColorStop::from((0.0, vcolor(top))), ColorStop::from((1.0, vcolor(bottom)))]);
        let ctx = &mut self.p.ctx;
        ctx.set_transform(self.root);
        ctx.set_paint(gradient);
        ctx.fill_path(&vk::RoundedRect::from_rect(vrect(rect), radius.max(0.0)).to_path(tolerance));
    }

    fn fill_polygon(&mut self, points: &[Point], color: Color) {
        let Some((first, rest)) = points.split_first() else { return };
        let mut path = vk::BezPath::new();
        path.move_to(vpoint(*first));
        for p in rest {
            path.line_to(vpoint(*p));
        }
        path.close_path();
        self.solid(color).fill_path(&path);
    }

    fn push_clip(&mut self, rect: Rect) {
        let ctx = &mut self.p.ctx;
        ctx.set_transform(self.root);
        ctx.push_clip_layer(&vrect(rect).to_path(TOLERANCE));
        self.clips += 1;
    }

    fn pop_clip(&mut self) {
        if self.clips > 0 {
            self.p.ctx.pop_layer();
            self.clips -= 1;
        }
    }

    fn draw_text(&mut self, layout: &super::text::Layout, top_left: Point, color: Color) {
        let scale = self.ppp;
        let p = &mut *self.p;
        let raster = p.texts.entry((layout.id(), scale.to_bits())).or_insert_with(|| TextRaster::new(&layout.0, scale, &mut p.resources));
        raster.used = true;
        let origin = ((top_left.x * scale).round() + raster.offset.0, (top_left.y * scale).round() + raster.offset.1);
        let ctx = &mut p.ctx;
        ctx.set_transform(Affine::IDENTITY);
        for (pixmap, tint) in [(&raster.mono, Some(color)), (&raster.color, None)] {
            let Some(pixmap) = pixmap else { continue };
            let sampler = ImageSampler { quality: ImageQuality::Low, ..Default::default() };
            ctx.set_paint(vello_cpu::Image { image: ImageSource::Pixmap(pixmap.clone()), sampler });
            ctx.set_paint_transform(Affine::translate(origin));
            ctx.set_tint(tint.map(|c| Tint { color: vcolor(c), mode: TintMode::AlphaMask }));
            ctx.fill_rect(&vk::Rect::from_origin_size(origin, (pixmap.width() as f64, pixmap.height() as f64)));
            ctx.reset_tint();
            ctx.reset_paint_transform();
        }
    }

    fn draw_image(&mut self, image: &Image, dst: Rect) {
        let [iw, ih] = image.size();
        if iw == 0 || ih == 0 || dst.is_empty() || iw > u16::MAX as u32 || ih > u16::MAX as u32 {
            return;
        }
        let cached = self.p.images.entry(image.id()).or_insert_with(|| CachedImage::new(image, iw as u16, ih as u16));
        cached.used = true;
        let pixmap = cached.pixmap.clone();
        // Physical px: the image's pixels mapped onto the destination rect.
        let d = dst.scale(self.ppp);
        let ctx = &mut self.p.ctx;
        ctx.set_transform(Affine::IDENTITY);
        let sampler = ImageSampler { quality: ImageQuality::Medium, ..Default::default() };
        ctx.set_paint(vello_cpu::Image { image: ImageSource::Pixmap(pixmap), sampler });
        ctx.set_paint_transform(Affine::translate((d.x0, d.y0)) * Affine::scale_non_uniform(d.width() / iw as f64, d.height() / ih as f64));
        ctx.fill_rect(&vrect(d));
        ctx.reset_paint_transform();
    }
}
