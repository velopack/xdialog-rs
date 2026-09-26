//! A softbuffer window surface (Linux/BSD: the software backend's frames; macOS: the CoreGraphics
//! backend draws straight into the buffer). A failed present recreates the surface and retries
//! once (surface lost).

use std::num::NonZeroU32;
use std::rc::Rc;

use winit::window::Window;

use super::DrawError;

fn err(what: &str, e: impl std::fmt::Display) -> DrawError {
    DrawError::Backend(format!("softbuffer {what}: {e}"))
}

pub(crate) struct Surface {
    /// This surface's own context (kept to recreate a lost surface).
    context: softbuffer::Context<Rc<Window>>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    /// Size the surface was last resized to (`[0, 0]` = not yet / must resize again).
    size: [u32; 2],
}

impl Surface {
    pub(crate) fn new(window: &Rc<Window>) -> Result<Self, DrawError> {
        let context = softbuffer::Context::new(window.clone()).map_err(|e| err("context", e))?;
        let surface = softbuffer::Surface::new(&context, window.clone()).map_err(|e| err("surface", e))?;
        Ok(Surface { context, surface, size: [0, 0] })
    }

    /// Present a frame of `size` (physical px, non-zero): `draw` fills the whole buffer
    /// (`size[0] * size[1]` pixels, row-major, softbuffer's `0x00RRGGBB`; its previous contents
    /// are undefined). On failure the surface is recreated and `draw` called once more.
    pub(crate) fn present_with(&mut self,
                               size: [u32; 2],
                               mut draw: impl FnMut(&mut [u32]) -> Result<(), DrawError>)
                               -> Result<(), DrawError> {
        let Err(e) = self.try_present(size, &mut draw) else { return Ok(()) };
        let window = self.surface.window().clone();
        self.surface =
            softbuffer::Surface::new(&self.context, window).map_err(|e2| DrawError::Backend(format!("{e}; recreating the surface: {e2}")))?;
        self.size = [0, 0];
        self.try_present(size, &mut draw).map_err(|e2| DrawError::Backend(format!("{e}; after recreating the surface: {e2}")))
    }

    /// Present opaque RGBA8 pixels of `size` (non-zero).
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub(crate) fn present_rgba(&mut self, size: [u32; 2], rgba: &[u8]) -> Result<(), DrawError> {
        self.present_with(size, |buffer| {
                if buffer.len() * 4 != rgba.len() {
                    return Err(DrawError::Backend(format!("{} bytes of RGBA for {} pixels", rgba.len(), buffer.len())));
                }
                // RGBA -> softbuffer's 0x00RRGGBB.
                for (dst, p) in buffer.iter_mut().zip(rgba.chunks_exact(4)) {
                    *dst = u32::from_be_bytes([0, p[0], p[1], p[2]]);
                }
                Ok(())
            })
    }

    fn try_present(&mut self, size: [u32; 2], draw: &mut impl FnMut(&mut [u32]) -> Result<(), DrawError>) -> Result<(), DrawError> {
        let [w, h] = size;
        if self.size != size {
            let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else {
                return Err(DrawError::Backend("zero-sized surface".into()));
            };
            if let Err(e) = self.surface.resize(nw, nh) {
                self.size = [0, 0];
                return Err(err("resize", e));
            }
            self.size = size;
        }
        let mut buffer = self.surface.buffer_mut().map_err(|e| err("buffer", e))?;
        if buffer.len() != w as usize * h as usize {
            self.size = [0, 0];
            return Err(DrawError::Backend(format!("softbuffer returned {} pixels for a {w}x{h} surface", buffer.len())));
        }
        draw(&mut buffer)?;
        buffer.present().map_err(|e| err("present", e))
    }
}
