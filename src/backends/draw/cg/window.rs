//! The surfaces: [`WindowSurface`] draws with a CG bitmap context straight into softbuffer's
//! buffer (its own `0x00RRGGBB` format: no copy, no conversion), or, for a translucent window
//! ([`WindowSurface::translucent`]: a behind-window material shows through), into an owned
//! premultiplied-alpha buffer shown as the contents of its own `CALayer` (softbuffer's layer is
//! always opaque); `MemorySurface` draws opaque into an owned buffer and converts that to RGBA on
//! present.
//!
//! Scale: the buffers are in physical px and the layers' `contentsScale` is the window's backing
//! scale, so they show 1:1 on the screen's pixels; the only logical to physical scaling is the
//! canvas's `scale_ctm(ppp)`.

use std::rc::Rc;

use winit::window::Window;

use super::canvas::Painter;
use super::text::Text;
use crate::backends::draw::softbuffer_surface;
use crate::backends::draw::{list, DrawError, Frame};

/// Presents into a window: through softbuffer (opaque), or through its own layer (translucent).
pub(crate) struct WindowSurface {
    target: Presenter,
    painter: Painter,
}

enum Presenter {
    Soft(softbuffer_surface::Surface),
    Layer(layer::LayerTarget),
}

impl crate::backends::draw::WindowTarget for WindowSurface {
    fn new(window: &Rc<Window>, text: &Rc<Text>) -> Result<Self, DrawError> {
        Ok(WindowSurface { target: Presenter::Soft(softbuffer_surface::Surface::new(window)?),
                           painter: Painter::new(text.color_space.clone(), Painter::system_font_smoothing()) })
    }
}

impl WindowSurface {
    /// A surface whose frames keep their alpha (for a transparent window over an
    /// `NSVisualEffectView`): a translucent clear colour lets the material show through.
    pub(crate) fn translucent(window: &Rc<Window>, text: &Rc<Text>) -> Result<Self, DrawError> {
        Ok(WindowSurface { target: Presenter::Layer(layer::LayerTarget::new(window, text.color_space.clone())?),
                           painter: Painter::new(text.color_space.clone(), Painter::system_font_smoothing()) })
    }
}

impl crate::backends::draw::Surface for WindowSurface {
    fn present(&mut self, frame: &Frame<'_>) -> Result<(), DrawError> {
        if list::is_zero_size(frame.size_px) {
            return Ok(()); // minimised / not laid out yet
        }
        let painter = &mut self.painter;
        match &mut self.target {
            Presenter::Soft(surface) => surface.present_with(frame.size_px, |buffer| painter.draw(buffer, frame, false)),
            Presenter::Layer(layer) => layer.present(painter, frame),
        }
    }
}

mod layer {
    //! A `CALayer` added over the window view's own layer, showing each frame as a premultiplied
    //! `CGImage`.

    use std::ptr;
    use std::rc::Rc;

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::NSView;
    use objc2_core_foundation::{CFData, CFRetained, CGPoint};
    use objc2_core_graphics::{
        CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo,
    };
    use objc2_quartz_core::{kCAGravityTopLeft, CALayer, CATransaction};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    use super::Painter;
    use crate::backends::draw::{DrawError, Frame};

    pub(super) struct LayerTarget {
        root: Retained<CALayer>,
        layer: Retained<CALayer>,
        space: CFRetained<CGColorSpace>,
        /// The frame buffer, premultiplied `0xAARRGGBB` per pixel.
        pixels: Vec<u32>,
    }

    impl LayerTarget {
        pub(super) fn new(window: &Rc<Window>, space: CFRetained<CGColorSpace>) -> Result<Self, DrawError> {
            let handle = window.window_handle().map_err(|e| DrawError::Backend(format!("cg: no window handle: {e}")))?;
            let RawWindowHandle::AppKit(h) = handle.as_raw() else {
                return Err(DrawError::Backend("cg: not an AppKit window".into()));
            };
            // SAFETY: winit's AppKit handle holds the window's `NSView`, valid while the window
            // lives; this runs on the main thread (winit's event loop thread on macOS).
            let view: &NSView = unsafe { h.ns_view.cast().as_ref() };
            view.setWantsLayer(true);
            let root = view.layer().ok_or_else(|| DrawError::Backend("cg: the view has no layer".into()))?;
            let layer = CALayer::new();
            layer.setAnchorPoint(CGPoint::new(0.0, 0.0));
            layer.setGeometryFlipped(true);
            layer.setOpaque(false);
            // SAFETY: a constant string provided by QuartzCore.
            layer.setContentsGravity(unsafe { kCAGravityTopLeft });
            root.addSublayer(&layer);
            Ok(LayerTarget { root, layer, space, pixels: Vec::new() })
        }

        pub(super) fn present(&mut self, painter: &mut Painter, frame: &Frame<'_>) -> Result<(), DrawError> {
            let [w, h] = frame.size_px;
            let (w, h) = (w as usize, h as usize);
            self.pixels.resize(w * h, 0);
            painter.draw(&mut self.pixels, frame, true)?;
            // SAFETY: plain `u32`s viewed as their bytes.
            let bytes = unsafe { std::slice::from_raw_parts(self.pixels.as_ptr().cast::<u8>(), self.pixels.len() * 4) };
            // CFData copies the frame: the layer keeps showing its image while the next is drawn.
            let data = CFData::from_bytes(bytes);
            let provider =
                CGDataProvider::with_cf_data(Some(&data)).ok_or_else(|| DrawError::Backend("cg: CGDataProviderCreateWithCFData failed".into()))?;
            let info = CGBitmapInfo(CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0);
            // SAFETY: 8 bits per component, 32 per pixel, `w * 4` bytes per row: exactly the
            // `w * h * 4` bytes the provider holds; `decode` may be null (no remapping).
            let image = unsafe {
                CGImage::new(w,
                             h,
                             8,
                             32,
                             w * 4,
                             Some(&self.space),
                             info,
                             Some(&provider),
                             ptr::null(),
                             false,
                             CGColorRenderingIntent::RenderingIntentDefault)
            }.ok_or_else(|| DrawError::Backend("cg: CGImageCreate failed".into()))?;
            // No implicit animations (a contents change would cross-fade).
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            self.layer.setFrame(self.root.bounds());
            self.layer.setContentsScale(frame.ppp);
            // SAFETY: a `CGImage` is a valid `contents` object (a CF type bridged to an object).
            unsafe { self.layer.setContents(Some(&*(CFRetained::as_ptr(&image).as_ptr() as *const AnyObject))) };
            CATransaction::commit();
            Ok(())
        }
    }

    impl Drop for LayerTarget {
        fn drop(&mut self) {
            self.layer.removeFromSuperlayer();
        }
    }
}

/// Renders into memory; the last frame is readable as opaque RGBA8.
#[cfg(any(test, feature = "_test-hooks"))]
pub(crate) struct MemorySurface {
    painter: Painter,
    /// The frame buffer, `0x00RRGGBB` per pixel.
    pixels: Vec<u32>,
    rgba: Vec<u8>,
    /// Size of the last frame (`[0, 0]`: none yet).
    size: [u32; 2],
}

#[cfg(any(test, feature = "_test-hooks"))]
impl crate::backends::draw::MemoryTarget for MemorySurface {
    fn new(text: &Rc<Text>) -> Result<Self, DrawError> {
        // Smoothing always on (AppKit's default, what a window shows unless the user turned it
        // off): renders look like the real dialog and do not depend on the user's setting.
        Ok(MemorySurface { painter: Painter::new(text.color_space.clone(), true), pixels: Vec::new(), rgba: Vec::new(), size: [0, 0] })
    }

    fn read_rgba(&self) -> Option<(u32, u32, &[u8])> {
        (!list::is_zero_size(self.size)).then(|| (self.size[0], self.size[1], &self.rgba[..]))
    }
}

#[cfg(any(test, feature = "_test-hooks"))]
impl crate::backends::draw::Surface for MemorySurface {
    fn present(&mut self, frame: &Frame<'_>) -> Result<(), DrawError> {
        if list::is_zero_size(frame.size_px) {
            return Ok(()); // the previous image is kept
        }
        let n = frame.size_px[0] as usize * frame.size_px[1] as usize;
        self.pixels.resize(n, 0);
        self.painter.draw(&mut self.pixels, frame, false)?;
        self.rgba.clear();
        self.rgba.extend(self.pixels.iter().flat_map(|&p| {
                                               let [_, r, g, b] = p.to_be_bytes();
                                               [r, g, b, 255]
                                           }));
        self.size = frame.size_px;
        Ok(())
    }
}
