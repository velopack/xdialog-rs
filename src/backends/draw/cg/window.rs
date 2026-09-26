//! The surfaces: [`WindowSurface`] draws with a CG bitmap context straight into softbuffer's
//! buffer (its own `0x00RRGGBB` format: no copy, no conversion); `MemorySurface` draws the same way
//! into an owned buffer and converts that to RGBA on present.
//!
//! Scale: softbuffer's buffer is in physical px and its CALayer follows the window's backing scale
//! (`contentsScale`), so the layer shows it 1:1 on the screen's pixels; the only logical to
//! physical scaling is the canvas's `scale_ctm(ppp)`.

use std::rc::Rc;

use winit::window::Window;

use super::canvas::Painter;
use super::text::Text;
use crate::backends::draw::softbuffer_surface;
use crate::backends::draw::{list, DrawError, Frame};

/// Presents into a window through softbuffer.
pub(crate) struct WindowSurface {
    surface: softbuffer_surface::Surface,
    painter: Painter,
}

impl crate::backends::draw::WindowTarget for WindowSurface {
    fn new(window: &Rc<Window>, text: &Rc<Text>) -> Result<Self, DrawError> {
        Ok(WindowSurface { surface: softbuffer_surface::Surface::new(window)?, painter: Painter::new(text.color_space.clone()) })
    }
}

impl crate::backends::draw::Surface for WindowSurface {
    fn present(&mut self, frame: &Frame<'_>) -> Result<(), DrawError> {
        if list::is_zero_size(frame.size_px) {
            return Ok(()); // minimised / not laid out yet
        }
        let painter = &mut self.painter;
        self.surface.present_with(frame.size_px, |buffer| painter.draw(buffer, frame))
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
        Ok(MemorySurface { painter: Painter::new(text.color_space.clone()), pixels: Vec::new(), rgba: Vec::new(), size: [0, 0] })
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
        self.painter.draw(&mut self.pixels, frame)?;
        self.rgba.clear();
        self.rgba.extend(self.pixels.iter().flat_map(|&p| {
                                               let [_, r, g, b] = p.to_be_bytes();
                                               [r, g, b, 255]
                                           }));
        self.size = frame.size_px;
        Ok(())
    }
}
