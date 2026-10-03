//! The software drawing backend (Linux and other Unix): vello_cpu draws, cosmic-text shapes
//! (bidi, line breaking, font fallback), with no C dependencies and no fontconfig. Windows
//! present through softbuffer.
//!
//! - `fonts.rs`: the process-wide font database (bundled Ubuntu + a background system scan).
//! - `text.rs`: [`Text`] (layouts, memoized shaping).
//! - `canvas.rs`: [`Canvas`] on vello_cpu (and the text rasters).

mod canvas;
pub(crate) mod fonts;
mod text;

use std::rc::Rc;

use winit::window::Window;

pub(crate) use canvas::Canvas;
use canvas::Painter;
pub(crate) use text::Text;

use super::softbuffer_surface;
use super::{DrawError, Frame};

/// Names the golden directory.
pub const NAME: &str = "soft";

pub(crate) struct WindowSurface {
    surface: softbuffer_surface::Surface,
    painter: Painter,
}

impl super::WindowTarget for WindowSurface {
    fn new(window: &Rc<Window>, _text: &Rc<Text>) -> Result<Self, DrawError> {
        Ok(WindowSurface { surface: softbuffer_surface::Surface::new(window)?, painter: Painter::new() })
    }
}

impl super::Surface for WindowSurface {
    fn present_nonzero(&mut self, frame: &Frame<'_>) -> Result<(), DrawError> {
        self.painter.draw(frame)?;
        self.surface.present_rgba(frame.size_px, self.painter.pixmap().data_as_u8_slice())
    }
}

/// Renders into memory; the last frame is readable as opaque RGBA8.
#[cfg(any(test, feature = "_test-hooks"))]
pub(crate) struct MemorySurface {
    painter: Painter,
    /// Size of the last frame (`[0, 0]`: none yet).
    size: [u32; 2],
}

#[cfg(any(test, feature = "_test-hooks"))]
impl super::MemoryTarget for MemorySurface {
    fn new(_text: &Rc<Text>) -> Result<Self, DrawError> {
        Ok(MemorySurface { painter: Painter::new(), size: [0, 0] })
    }

    fn read_rgba(&self) -> Option<(u32, u32, &[u8])> {
        // The clear colour is opaque, so the premultiplied pixels are plain opaque RGBA.
        (!super::list::is_zero_size(self.size)).then(|| (self.size[0], self.size[1], self.painter.pixmap().data_as_u8_slice()))
    }
}

#[cfg(any(test, feature = "_test-hooks"))]
impl super::Surface for MemorySurface {
    fn present_nonzero(&mut self, frame: &Frame<'_>) -> Result<(), DrawError> {
        self.painter.draw(frame)?;
        self.size = [self.painter.pixmap().width() as u32, self.painter.pixmap().height() as u32];
        Ok(())
    }
}
