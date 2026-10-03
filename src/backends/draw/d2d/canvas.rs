//! [`Canvas`] on a Direct2D device context, and the device-bound resources it draws with.

use std::collections::HashMap;

use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::core::Interface;
use windows_numerics::Vector2;

use super::text::Text;
use crate::backends::draw::{list, Color, DrawError, Frame, Image, LineCap, Point, Rect};

/// Everything bound to one render target's device: dropped together when the device is lost.
pub(crate) struct DeviceRes {
    factory: ID2D1Factory,
    brush: ID2D1SolidColorBrush,
    round: ID2D1StrokeStyle,
    /// Uploaded images by `Image::id`, and whether the current frame drew them.
    images: HashMap<u64, (ID2D1Bitmap1, bool)>,
    /// RGBA -> BGRA scratch buffer.
    swizzle: Vec<u8>,
}

impl DeviceRes {
    pub(crate) fn new(dc: &ID2D1DeviceContext, text: &Text) -> windows::core::Result<Self> {
        let props = D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND,
                                                   endCap: D2D1_CAP_STYLE_ROUND,
                                                   dashCap: D2D1_CAP_STYLE_ROUND,
                                                   lineJoin: D2D1_LINE_JOIN_ROUND,
                                                   miterLimit: 10.0,
                                                   dashStyle: D2D1_DASH_STYLE_SOLID,
                                                   dashOffset: 0.0 };
        // SAFETY: plain resource creation on live interfaces.
        unsafe {
            Ok(DeviceRes { factory: text.d2d.cast()?,
                           brush: dc.CreateSolidColorBrush(&color_f(Color::BLACK), None)?,
                           round: ID2D1Factory::CreateStrokeStyle(&text.d2d, &props, None)?,
                           images: HashMap::new(),
                           swizzle: Vec::new() })
        }
    }
}

fn color_f(c: Color) -> D2D1_COLOR_F {
    let [r, g, b, a] = c.to_straight_f32();
    D2D1_COLOR_F { r, g, b, a }
}

fn rect_f(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F { left: r.x0 as f32, top: r.y0 as f32, right: r.x1 as f32, bottom: r.y1 as f32 }
}

fn rounded(r: Rect, radius: f64) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT { rect: rect_f(r), radiusX: radius as f32, radiusY: radius as f32 }
}

fn vec2(p: Point) -> Vector2 {
    Vector2 { X: p.x as f32, Y: p.y as f32 }
}

/// Draw `frame` on `dc` (a render target cast to a device context). Images not drawn in the
/// frame are released afterwards. Any EndDraw failure (including `D2DERR_RECREATE_TARGET`) is
/// returned: the caller recreates the target.
pub(crate) fn draw_frame(dc: &ID2D1DeviceContext, dev: &mut DeviceRes, text: &Text, frame: &Frame<'_>) -> Result<(), DrawError> {
    let dpi = (96.0 * frame.ppp) as f32;
    // SAFETY: plain calls on a live device context; BeginDraw is always paired with EndDraw.
    unsafe {
        dc.BeginDraw();
        dc.SetDpi(dpi, dpi);
        dc.SetTransform(&windows_numerics::Matrix3x2::identity());
        dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        dc.SetTextRenderingParams(&text.params);
    }
    list::replay(&mut Canvas { dc, dev }, frame);
    // SAFETY: as above.
    let ended = unsafe { dc.EndDraw(None, None) };
    dev.images.retain(|_, (_, used)| std::mem::take(used));
    ended.map_err(super::backend("EndDraw"))
}

/// A frame being drawn into a Direct2D device context (between BeginDraw and EndDraw), in DIPs
/// (= logical px: the surface set the context's DPI to 96 * ppp).
pub(crate) struct Canvas<'a> {
    dc: &'a ID2D1DeviceContext,
    dev: &'a mut DeviceRes,
}

impl Canvas<'_> {
    fn brush(&self, color: Color) -> &ID2D1SolidColorBrush {
        // SAFETY: plain setter on a live brush.
        unsafe { self.dev.brush.SetColor(&color_f(color)) };
        &self.dev.brush
    }

    /// The device bitmap of `image` (uploaded on first use).
    fn bitmap(&mut self, image: &Image) -> windows::core::Result<ID2D1Bitmap1> {
        if let Some((bmp, used)) = self.dev.images.get_mut(&image.id()) {
            *used = true;
            return Ok(bmp.clone());
        }
        let [w, h] = image.size();
        let buf = &mut self.dev.swizzle;
        buf.clear();
        buf.extend(image.rgba().as_chunks::<4>().0.iter().flat_map(|p| [p[2], p[1], p[0], p[3]]));
        let props = D2D1_BITMAP_PROPERTIES1 { pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM,
                                                                               alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                                              dpiX: 96.0,
                                              dpiY: 96.0,
                                              bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
                                              ..Default::default() };
        // SAFETY: `buf` holds `h` rows of `w * 4` bytes; Direct2D copies it.
        let bmp = unsafe { self.dc.CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(buf.as_ptr().cast()), w * 4, &props)? };
        self.dev.images.insert(image.id(), (bmp.clone(), true));
        Ok(bmp)
    }
}

// SAFETY (every block below): plain drawing calls on a live device context between BeginDraw and
// EndDraw, with pointers to locals.
impl crate::backends::draw::Canvas for Canvas<'_> {
    fn clear(&mut self, color: Color) {
        unsafe { self.dc.Clear(Some(&color_f(color))) };
    }

    fn fill_rect(&mut self, rect: Rect, radius: f64, color: Color) {
        let brush = self.brush(color);
        unsafe {
            if radius > 0.0 {
                self.dc.FillRoundedRectangle(&rounded(rect, radius), brush);
            } else {
                self.dc.FillRectangle(&rect_f(rect), brush);
            }
        }
    }

    fn stroke_rect(&mut self, rect: Rect, radius: f64, width: f64, color: Color) {
        let brush = self.brush(color);
        unsafe {
            if radius > 0.0 {
                self.dc.DrawRoundedRectangle(&rounded(rect, radius), brush, width as f32, None);
            } else {
                self.dc.DrawRectangle(&rect_f(rect), brush, width as f32, None);
            }
        }
    }

    fn fill_circle(&mut self, center: Point, radius: f64, color: Color) {
        let ellipse = D2D1_ELLIPSE { point: vec2(center), radiusX: radius as f32, radiusY: radius as f32 };
        unsafe { self.dc.FillEllipse(&ellipse, self.brush(color)) };
    }

    fn line(&mut self, from: Point, to: Point, width: f64, cap: LineCap, color: Color) {
        let style = match cap {
            LineCap::Butt => None,
            LineCap::Round => Some(&self.dev.round),
        };
        unsafe { self.dc.DrawLine(vec2(from), vec2(to), self.brush(color), width as f32, style) };
    }

    fn fill_rect_gradient(&mut self, rect: Rect, radius: f64, top: Color, bottom: Color) {
        let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: color_f(top) }, D2D1_GRADIENT_STOP { position: 1.0, color: color_f(bottom) }];
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: vec2(Point::new(rect.x0, rect.y0)), endPoint: vec2(Point::new(rect.x0, rect.y1)) };
        let brush = unsafe {
            ID2D1RenderTarget::CreateGradientStopCollection(self.dc, &stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
                .and_then(|stops| self.dc.CreateLinearGradientBrush(&props, None, &stops))
        };
        match brush {
            Ok(brush) => unsafe {
                if radius > 0.0 {
                    self.dc.FillRoundedRectangle(&rounded(rect, radius), &brush);
                } else {
                    self.dc.FillRectangle(&rect_f(rect), &brush);
                }
            },
            Err(e) => warn!("xdialog: could not create a Direct2D gradient: {e}"),
        }
    }

    fn fill_polygon(&mut self, points: &[Point], color: Color) {
        let Some((first, rest)) = points.split_first() else { return };
        let rest: Vec<Vector2> = rest.iter().map(|p| vec2(*p)).collect();
        let geometry = unsafe {
            (|| -> windows::core::Result<ID2D1PathGeometry> {
                let geometry = self.dev.factory.CreatePathGeometry()?;
                let sink = geometry.Open()?;
                sink.SetFillMode(D2D1_FILL_MODE_WINDING);
                sink.BeginFigure(vec2(*first), D2D1_FIGURE_BEGIN_FILLED);
                sink.AddLines(&rest);
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.Close()?;
                Ok(geometry)
            })()
        };
        match geometry {
            Ok(geometry) => unsafe { self.dc.FillGeometry(&geometry, self.brush(color), None) },
            Err(e) => warn!("xdialog: could not create a Direct2D path: {e}"),
        }
    }

    fn push_clip(&mut self, rect: Rect) {
        unsafe { self.dc.PushAxisAlignedClip(&rect_f(rect), D2D1_ANTIALIAS_MODE_ALIASED) };
    }

    fn pop_clip(&mut self) {
        unsafe { self.dc.PopAxisAlignedClip() };
    }

    fn draw_text(&mut self, layout: &crate::backends::draw::Layout, top_left: Point, color: Color) {
        let Some(l) = &layout.0.layout else { return };
        unsafe { self.dc.DrawTextLayout(vec2(top_left), l, self.brush(color), D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT) };
    }

    fn draw_image(&mut self, image: &Image, dst: Rect) {
        match self.bitmap(image) {
            Ok(bmp) => unsafe { self.dc.DrawBitmap(&bmp, Some(&rect_f(dst)), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None) },
            Err(e) => warn!("xdialog: could not upload an image to Direct2D: {e}"),
        }
    }
}
