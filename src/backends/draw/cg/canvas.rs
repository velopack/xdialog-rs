//! [`Canvas`] on a `CGContext`, and the per-surface [`Painter`] that sets a bitmap context up
//! for a frame and replays the display list onto it.
//!
//! The context's user space is flipped and scaled once per frame (`translate(0, h)`,
//! `scale(ppp, -ppp)`), so everything is drawn in logical px with y pointing down. Text and images
//! flip back locally (never through the text matrix: `CTLineDraw` misplaces diacritics under a
//! flipped one).

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;

use objc2_core_foundation::{CFData, CFRetained, CGAffineTransform, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGContext, CGDataProvider, CGImage, CGImageAlphaInfo,
    CGGradient, CGGradientDrawingOptions, CGImageByteOrderInfo, CGInterpolationQuality, CGLineCap, CGPath,
};

use crate::backends::draw::{list, Color, DrawError, Frame, Image, LineCap, Point, Rect};

fn cg_rect(r: Rect) -> CGRect {
    CGRect::new(CGPoint::new(r.x0, r.y0), CGSize::new(r.width(), r.height()))
}

/// Whether `v` is positive and finite (not NaN).
fn positive(v: f64) -> bool {
    v > 0.0 && v.is_finite()
}

/// Whether `r` has a positive, finite area.
fn drawable(r: Rect) -> bool {
    positive(r.width()) && positive(r.height())
}

/// A corner radius `CGPathCreateWithRoundedRect` accepts for `r` (it requires
/// `0 <= 2 * radius <= min(width, height)`).
fn corner(r: Rect, radius: f64) -> f64 {
    let max = r.width().min(r.height()) / 2.0;
    if radius > 0.0 {
        radius.min(max).max(0.0)
    } else {
        0.0
    }
}

const IDENTITY: CGAffineTransform = CGAffineTransform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };

/// Images uploaded as `CGImage`s by id; entries not drawn in a frame are dropped after it.
#[derive(Default)]
pub(crate) struct ImageCache {
    images: HashMap<u64, (CFRetained<CGImage>, bool)>,
}

impl ImageCache {
    fn get(&mut self, image: &Image, space: &CGColorSpace) -> Option<CFRetained<CGImage>> {
        if let Some((cg, used)) = self.images.get_mut(&image.id()) {
            *used = true;
            return Some(cg.clone());
        }
        let cg = new_image(image, space)?;
        self.images.insert(image.id(), (cg.clone(), true));
        Some(cg)
    }

    fn sweep(&mut self) {
        self.images.retain(|_, (_, used)| std::mem::replace(used, false));
    }
}

/// A `CGImage` over a copy of the image's premultiplied RGBA8 pixels.
fn new_image(image: &Image, space: &CGColorSpace) -> Option<CFRetained<CGImage>> {
    let [w, h] = image.size();
    let (w, h) = (w as usize, h as usize);
    if w == 0 || h == 0 || image.rgba().len() != w * h * 4 {
        return None;
    }
    // CFData copies the bytes, so the image owns its pixels whatever CG retains it for.
    let data = CFData::from_bytes(image.rgba());
    let provider = CGDataProvider::with_cf_data(Some(&data))?;
    // Byte order default + alpha last: R, G, B, A bytes, premultiplied.
    let info = CGBitmapInfo(CGImageAlphaInfo::PremultipliedLast.0);
    // SAFETY: 8 bits per component, 32 per pixel, `w * 4` bytes per row: exactly the `w * h * 4`
    // bytes the provider holds (checked above); `decode` may be null (no remapping).
    unsafe {
        CGImage::new(w,
                     h,
                     8,
                     32,
                     w * 4,
                     Some(space),
                     info,
                     Some(&provider),
                     ptr::null(),
                     true,
                     CGColorRenderingIntent::RenderingIntentDefault)
    }
}

/// Draws frames into CG bitmap contexts over caller-provided pixel buffers (one per surface).
pub(crate) struct Painter {
    space: CFRetained<CGColorSpace>,
    images: ImageCache,
}

impl Painter {
    pub(crate) fn new(space: CFRetained<CGColorSpace>) -> Self {
        Painter { space, images: ImageCache::default() }
    }

    /// Draw `frame` into `pixels`: `frame.size_px` (non-zero), row-major top row first, each pixel
    /// a native-endian `0x00RRGGBB` (softbuffer's format; `NoneSkipFirst` + 32-bit little
    /// endian in CG terms), or with `alpha` a premultiplied `0xAARRGGBB` (`PremultipliedFirst`:
    /// a translucent clear colour stays translucent). The previous contents do not matter (the
    /// frame is cleared first).
    pub(crate) fn draw(&mut self, pixels: &mut [u32], frame: &Frame<'_>, alpha: bool) -> Result<(), DrawError> {
        let [w, h] = frame.size_px;
        let (w, h) = (w as usize, h as usize);
        if pixels.len() != w * h {
            return Err(DrawError::Backend(format!("cg: {} pixels for a {w}x{h} frame", pixels.len())));
        }
        let alpha_info = if alpha { CGImageAlphaInfo::PremultipliedFirst } else { CGImageAlphaInfo::NoneSkipFirst };
        let info = alpha_info.0 | CGImageByteOrderInfo::Order32Little.0;
        // SAFETY: `pixels` holds `w * h` u32 = `h` rows of `w * 4` bytes, u32-aligned, and stays
        // mutably borrowed (untouched by anything else) until the context is released at the end
        // of this function, after which nothing draws into it.
        let cx = unsafe { CGBitmapContextCreate(pixels.as_mut_ptr().cast::<c_void>(), w, h, 8, w * 4, Some(&self.space), info) }
            .ok_or_else(|| DrawError::Backend(format!("cg: CGBitmapContextCreate failed for {w}x{h}")))?;
        let c = Some(&*cx);
        // Deterministic output, independent of the user's font smoothing setting.
        CGContext::set_should_antialias(c, true);
        CGContext::set_allows_font_smoothing(c, false);
        CGContext::set_should_smooth_fonts(c, false);
        CGContext::set_allows_font_subpixel_positioning(c, true);
        CGContext::set_should_subpixel_position_fonts(c, true);
        CGContext::set_allows_font_subpixel_quantization(c, true);
        CGContext::set_should_subpixel_quantize_fonts(c, true);
        CGContext::set_interpolation_quality(c, CGInterpolationQuality::High);
        // Bitmap memory is top row first, CG user space is y-up from the bottom: flip, then scale
        // logical to physical px.
        CGContext::translate_ctm(c, 0.0, h as CGFloat);
        CGContext::scale_ctm(c, frame.ppp, -frame.ppp);
        let mut canvas =
            Canvas { cx: &cx, space: &self.space, images: &mut self.images, size: [w as f64 / frame.ppp, h as f64 / frame.ppp] };
        list::replay(&mut canvas, frame);
        CGContext::flush(c);
        drop(cx);
        self.images.sweep();
        Ok(())
    }
}

/// A frame being drawn into a CG context (logical px, y down).
pub(crate) struct Canvas<'a> {
    cx: &'a CGContext,
    space: &'a CGColorSpace,
    images: &'a mut ImageCache,
    /// Logical size of the frame.
    size: [f64; 2],
}

impl Canvas<'_> {
    fn c(&self) -> Option<&CGContext> {
        Some(self.cx)
    }

    fn fill_color(&self, color: Color) {
        let [r, g, b, a] = color.to_straight_f32();
        CGContext::set_rgb_fill_color(self.c(), r as CGFloat, g as CGFloat, b as CGFloat, a as CGFloat);
    }

    fn stroke_color(&self, color: Color) {
        let [r, g, b, a] = color.to_straight_f32();
        CGContext::set_rgb_stroke_color(self.c(), r as CGFloat, g as CGFloat, b as CGFloat, a as CGFloat);
    }

    /// Replace the current path with `rect` (rounded by `radius`).
    fn rect_path(&self, rect: Rect, radius: f64) {
        let c = self.c();
        CGContext::begin_path(c);
        let radius = corner(rect, radius);
        if radius > 0.0 {
            // SAFETY: a null transform is allowed (identity); the radius satisfies
            // `0 <= 2 * radius <= min(width, height)` (`corner`).
            let path = unsafe { CGPath::with_rounded_rect(cg_rect(rect), radius, radius, ptr::null()) };
            CGContext::add_path(c, Some(&path));
        } else {
            CGContext::add_rect(c, cg_rect(rect));
        }
    }
}

impl crate::backends::draw::Canvas for Canvas<'_> {
    fn clear(&mut self, color: Color) {
        // A margin past every edge: the whole bitmap whatever the rounding of `size`. Cleared to
        // transparent first, so a translucent colour replaces the previous frame.
        let all = cg_rect(Rect::new(-1.0, -1.0, self.size[0] + 1.0, self.size[1] + 1.0));
        CGContext::clear_rect(self.c(), all);
        self.fill_color(color);
        CGContext::fill_rect(self.c(), all);
    }

    fn fill_rect(&mut self, rect: Rect, radius: f64, color: Color) {
        if !drawable(rect) {
            return;
        }
        self.fill_color(color);
        if corner(rect, radius) > 0.0 {
            self.rect_path(rect, radius);
            CGContext::fill_path(self.c());
        } else {
            CGContext::fill_rect(self.c(), cg_rect(rect));
        }
    }

    fn stroke_rect(&mut self, rect: Rect, radius: f64, width: f64, color: Color) {
        if !drawable(rect) || !positive(width) {
            return;
        }
        self.stroke_color(color);
        CGContext::set_line_width(self.c(), width);
        self.rect_path(rect, radius);
        CGContext::stroke_path(self.c());
    }

    fn fill_circle(&mut self, center: Point, radius: f64, color: Color) {
        if !positive(radius) {
            return;
        }
        self.fill_color(color);
        let rect = Rect::new(center.x - radius, center.y - radius, center.x + radius, center.y + radius);
        CGContext::fill_ellipse_in_rect(self.c(), cg_rect(rect));
    }

    fn line(&mut self, from: Point, to: Point, width: f64, cap: LineCap, color: Color) {
        if !positive(width) {
            return;
        }
        let c = self.c();
        self.stroke_color(color);
        CGContext::set_line_width(c, width);
        CGContext::set_line_cap(c, match cap {
            LineCap::Butt => CGLineCap::Butt,
            LineCap::Round => CGLineCap::Round,
        });
        CGContext::begin_path(c);
        CGContext::move_to_point(c, from.x, from.y);
        CGContext::add_line_to_point(c, to.x, to.y);
        CGContext::stroke_path(c);
    }

    fn fill_rect_gradient(&mut self, rect: Rect, radius: f64, top: Color, bottom: Color) {
        if !drawable(rect) {
            return;
        }
        let [r0, g0, b0, a0] = top.to_straight_f32();
        let [r1, g1, b1, a1] = bottom.to_straight_f32();
        let components: [CGFloat; 8] = [r0, g0, b0, a0, r1, g1, b1, a1].map(|v| v as CGFloat);
        let locations: [CGFloat; 2] = [0.0, 1.0];
        // SAFETY: two colours of four components (the RGB space plus alpha) and two locations.
        let Some(gradient) = (unsafe { CGGradient::with_color_components(Some(self.space), components.as_ptr(), locations.as_ptr(), 2) })
        else {
            return;
        };
        let c = self.c();
        CGContext::save_g_state(c);
        self.rect_path(rect, radius);
        CGContext::clip(c);
        CGContext::draw_linear_gradient(c,
                                        Some(&gradient),
                                        CGPoint::new(rect.x0, rect.y0),
                                        CGPoint::new(rect.x0, rect.y1),
                                        CGGradientDrawingOptions::empty());
        CGContext::restore_g_state(c);
    }

    fn fill_polygon(&mut self, points: &[Point], color: Color) {
        let Some((first, rest)) = points.split_first() else { return };
        let c = self.c();
        self.fill_color(color);
        CGContext::begin_path(c);
        CGContext::move_to_point(c, first.x, first.y);
        for p in rest {
            CGContext::add_line_to_point(c, p.x, p.y);
        }
        CGContext::close_path(c);
        CGContext::fill_path(c);
    }

    fn push_clip(&mut self, rect: Rect) {
        CGContext::save_g_state(self.c());
        // An empty rect clips everything away, as it should.
        CGContext::clip_to_rect(self.c(), cg_rect(Rect::new(rect.x0, rect.y0, rect.x1.max(rect.x0), rect.y1.max(rect.y0))));
    }

    fn pop_clip(&mut self) {
        CGContext::restore_g_state(self.c());
    }

    fn draw_text(&mut self, layout: &super::text::Layout, top_left: Point, color: Color) {
        let c = self.c();
        for line in &layout.0.lines {
            CGContext::save_g_state(c);
            self.fill_color(color);
            // Origin on the baseline, y up again for the glyphs.
            CGContext::translate_ctm(c, top_left.x + line.x, top_left.y + line.baseline);
            CGContext::scale_ctm(c, 1.0, -1.0);
            // The text matrix is not part of the graphics state: reset it for every line.
            CGContext::set_text_matrix(c, IDENTITY);
            CGContext::set_text_position(c, 0.0, 0.0);
            // SAFETY: a live line drawn into a live context.
            unsafe { line.line.draw(self.cx) };
            CGContext::restore_g_state(c);
        }
    }

    fn draw_image(&mut self, image: &Image, dst: Rect) {
        if !drawable(dst) {
            return;
        }
        let Some(cg) = self.images.get(image, self.space) else { return };
        let c = self.c();
        CGContext::save_g_state(c);
        // CGContextDrawImage puts the image's top row at the rect's top in y-up space: flip back
        // locally around the destination.
        CGContext::translate_ctm(c, dst.x0, dst.y1);
        CGContext::scale_ctm(c, 1.0, -1.0);
        CGContext::draw_image(c, CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(dst.width(), dst.height())), Some(&cg));
        CGContext::restore_g_state(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corner_radii_are_clamped() {
        let r = Rect::new(0.0, 0.0, 10.0, 4.0);
        assert_eq!(corner(r, 3.0), 2.0);
        assert_eq!(corner(r, 1.0), 1.0);
        assert_eq!(corner(r, -1.0), 0.0);
        assert_eq!(corner(r, f64::NAN), 0.0);
        assert!(!drawable(Rect::new(0.0, 0.0, 0.0, 4.0)));
        assert!(!drawable(Rect::new(0.0, 0.0, f64::NAN, 4.0)));
    }
}
