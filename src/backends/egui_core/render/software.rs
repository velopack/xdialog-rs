//! Software presenters: the vendored raster drawing into a softbuffer surface
//! (`SoftwarePresenter`) or into memory (`MemoryPresenter`, offscreen and tests).
//!
//! Both presenters run the exact same pixel pipeline (`rasterize`): fill with the clear colour,
//! draw egui's meshes with `ColorFieldOrder::Bgra`, so offscreen captures match live windows
//! byte for byte (apart from softbuffer's zeroed alpha byte).

use std::num::NonZeroU32;
use std::rc::Rc;

use winit::event_loop::OwnedDisplayHandle;
use winit::window::Window;

use super::raster::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
use super::{Presenter, RenderFrame};

/// The shared renderer configuration: BGRA output (softbuffer's `0x00RRGGBB` on little-endian),
/// no primitive cache (not part of the vendored subset).
fn new_raster() -> EguiSoftwareRender {
    EguiSoftwareRender::new(ColorFieldOrder::Bgra)
}

fn is_zero_size(size: [u32; 2]) -> bool {
    size[0] == 0 || size[1] == 0
}

/// Fill `px` (BGRA, `size` non-zero, `px.len() == w * h`) with the clear colour and draw the frame
/// over it. The presenters apply `frame.textures` up front (xdialog only uses the font atlas,
/// which is never freed, so freeing before drawing is safe).
fn rasterize(raster: &mut EguiSoftwareRender, px: &mut [[u8; 4]], frame: &RenderFrame<'_>) {
    let [w, h] = frame.size_px;
    let [r, g, b, a] = frame.clear.to_array();
    px.fill([b, g, r, a]);
    let mut buf = BufferMutRef::new(px, w as usize, h as usize);
    raster.render(&mut buf, frame.prims, &egui::TexturesDelta::default(), frame.ppp);
}

// ---------------------------------------------------------------------------------------------
// Memory presenter
// ---------------------------------------------------------------------------------------------

#[cfg(any(test, feature = "_test-hooks"))]
pub(crate) use memory::MemoryPresenter;

#[cfg(any(test, feature = "_test-hooks"))]
mod memory {
    use super::*;

    /// Renders into memory; the last frame is readable as opaque RGBA8 (`read_rgba`).
    pub(crate) struct MemoryPresenter {
        raster: EguiSoftwareRender,
        bgra: Vec<[u8; 4]>,
        rgba: Vec<u8>,
        size: [u32; 2],
    }

    impl MemoryPresenter {
        pub(crate) fn new() -> Self {
            Self { raster: new_raster(), bgra: Vec::new(), rgba: Vec::new(), size: [0, 0] }
        }

        #[cfg(test)]
        pub(in super::super) fn with_raster(raster: EguiSoftwareRender) -> Self {
            Self { raster, ..Self::new() }
        }
    }

    impl Presenter for MemoryPresenter {
        fn present(&mut self, frame: RenderFrame<'_>) -> Result<(), String> {
            self.raster.update_textures(frame.textures);
            if is_zero_size(frame.size_px) {
                return Ok(()); // the previous image is kept
            }
            let [w, h] = frame.size_px;
            let n = w as usize * h as usize;
            self.bgra.resize(n, [0; 4]);
            rasterize(&mut self.raster, &mut self.bgra, &frame);

            // BGRA -> RGBA; windows are opaque, so is the capture.
            self.rgba.clear();
            self.rgba.reserve(n * 4);
            self.rgba.extend(self.bgra.iter().flat_map(|p| [p[2], p[1], p[0], 255]));
            self.size = frame.size_px;
            Ok(())
        }

        fn read_rgba(&self) -> Option<(u32, u32, &[u8])> {
            if is_zero_size(self.size) {
                None
            } else {
                Some((self.size[0], self.size[1], &self.rgba))
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// softbuffer presenter
// ---------------------------------------------------------------------------------------------

/// Presents through a softbuffer surface (on the runtime's shared `softbuffer::Context`; the
/// surface keeps its window alive through the `Rc`).
pub(crate) struct SoftwarePresenter {
    /// A clone of the shared context, to recreate a lost surface.
    context: softbuffer::Context<OwnedDisplayHandle>,
    surface: softbuffer::Surface<OwnedDisplayHandle, Rc<Window>>,
    raster: EguiSoftwareRender,
    /// Size the surface was last resized to (`[0, 0]` = not yet / must resize again).
    surface_size: [u32; 2],
}

impl SoftwarePresenter {
    /// Create a surface for `window` on the shared `context`.
    pub(crate) fn new(context: &softbuffer::Context<OwnedDisplayHandle>, window: Rc<Window>) -> Result<Self, String> {
        let surface = softbuffer::Surface::new(context, window).map_err(|e| e.to_string())?;
        Ok(Self { context: context.clone(), surface, raster: new_raster(), surface_size: [0, 0] })
    }

    /// Resize (when needed), draw, mask the alpha byte, present.
    fn draw(&mut self, frame: &RenderFrame<'_>) -> Result<(), String> {
        let [w, h] = frame.size_px;
        if self.surface_size != frame.size_px {
            let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else {
                unreachable!("zero size handled by the caller");
            };
            if let Err(e) = self.surface.resize(nw, nh) {
                self.surface_size = [0, 0];
                return Err(e.to_string());
            }
            self.surface_size = frame.size_px;
        }

        let mut buffer = self.surface.buffer_mut().map_err(|e| e.to_string())?;
        let n = w as usize * h as usize;
        if buffer.len() != n {
            self.surface_size = [0, 0];
            return Err(format!("softbuffer returned {} pixels for a {w}x{h} surface", buffer.len()));
        }

        let words: &mut [u32] = &mut buffer;
        // SAFETY: `[u8; 4]` has the same size as `u32` and an alignment (1) that divides `u32`'s,
        // every bit pattern is valid for both, and `px` covers exactly the memory of `words`, which
        // is not touched again while `px` is alive (it is reborrowed from `buffer` only afterwards).
        let px: &mut [[u8; 4]] = unsafe { std::slice::from_raw_parts_mut(words.as_mut_ptr().cast::<[u8; 4]>(), words.len()) };
        rasterize(&mut self.raster, px, frame);

        // Pixels are bytes [B, G, R, A]: as a little-endian word that is 0xAARRGGBB. softbuffer
        // wants 0x00RRGGBB (the top byte must be zero); the raster leaves blended alpha there.
        mask_alpha(&mut buffer);
        buffer.present().map_err(|e| e.to_string())
    }
}

impl Presenter for SoftwarePresenter {
    fn present(&mut self, frame: RenderFrame<'_>) -> Result<(), String> {
        self.raster.update_textures(frame.textures);
        if is_zero_size(frame.size_px) {
            return Ok(()); // minimised / not laid out yet
        }
        let Err(e) = self.draw(&frame) else { return Ok(()) };
        // Surface lost: recreate it (the raster keeps its font atlas) and retry once.
        self.surface = softbuffer::Surface::new(&self.context, self.surface.window().clone()).map_err(|e2| format!("{e}; recreating the surface: {e2}"))?;
        self.surface_size = [0, 0];
        self.draw(&frame).map_err(|e2| format!("{e}; after recreating the surface: {e2}"))
    }
}

/// Bytes `[B, G, R, A]` -> softbuffer words `0x00RRGGBB` (native endian).
pub(super) fn mask_alpha(words: &mut [u32]) {
    for p in words {
        *p = u32::from_le(*p) & 0x00FF_FFFF;
    }
}
