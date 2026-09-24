//! Software presenters: the vendored raster drawing into a softbuffer surface
//! (`SoftwarePresenter`) or into memory (`MemoryPresenter`, offscreen and tests).
//!
//! Both presenters run the exact same pixel pipeline (`rasterize`): fill with the clear colour,
//! draw egui's meshes with `ColorFieldOrder::Bgra`, so offscreen captures match live windows
//! byte for byte (apart from softbuffer's zeroed alpha byte).

use std::num::NonZeroU32;

use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowHandle};

use super::raster::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
use super::{Presenter, RenderError, RenderFrame};

/// The shared renderer configuration: BGRA output (softbuffer's `0x00RRGGBB` on little-endian),
/// no primitive cache (not part of the vendored subset).
fn new_raster() -> EguiSoftwareRender {
    EguiSoftwareRender::new(ColorFieldOrder::Bgra)
}

fn is_zero_size(size: [u32; 2]) -> bool {
    size[0] == 0 || size[1] == 0
}

/// Fill `px` (BGRA, `size` non-zero, `px.len() == w * h`) with the clear colour and draw the frame
/// over it. Applies `frame.textures` to the renderer; the caller clears the delta.
fn rasterize(raster: &mut EguiSoftwareRender, px: &mut [[u8; 4]], frame: &RenderFrame<'_>) {
    let [w, h] = frame.size_px;
    let [r, g, b, a] = frame.clear.to_array();
    px.fill([b, g, r, a]);
    let mut buf = BufferMutRef::new(px, w as usize, h as usize);
    raster.render(&mut buf, frame.prims, frame.textures, frame.ppp);
}

// ---------------------------------------------------------------------------------------------
// Memory presenter
// ---------------------------------------------------------------------------------------------

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
    pub(super) fn with_raster(raster: EguiSoftwareRender) -> Self {
        Self { raster, ..Self::new() }
    }
}

impl Default for MemoryPresenter {
    fn default() -> Self {
        Self::new()
    }
}

impl Presenter for MemoryPresenter {
    fn present(&mut self, frame: RenderFrame<'_>) -> Result<(), RenderError> {
        if is_zero_size(frame.size_px) {
            // Nothing to draw (the previous image is kept), but the atlas must stay in sync.
            self.raster.update_textures(frame.textures);
            frame.textures.clear();
            return Ok(());
        }
        let [w, h] = frame.size_px;
        let n = w as usize * h as usize;
        self.bgra.resize(n, [0; 4]);
        rasterize(&mut self.raster, &mut self.bgra, &frame);
        frame.textures.clear();

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

// ---------------------------------------------------------------------------------------------
// softbuffer presenter
// ---------------------------------------------------------------------------------------------

/// Presents through a softbuffer surface.
///
/// Own loop: `D = OwnedDisplayHandle` (one shared `softbuffer::Context` per loop) and
/// `W = Rc<winit::Window>`. Host mode: `D = W = RawHandles` via [`SoftwarePresenter::from_raw`]
/// (one context per window, owned by the presenter).
pub(crate) struct SoftwarePresenter<D, W> {
    surface: softbuffer::Surface<D, W>,
    /// Keeps a presenter-owned context (host mode) alive as long as the surface.
    _context: Option<softbuffer::Context<D>>,
    raster: EguiSoftwareRender,
    /// Size the surface was last resized to (`[0, 0]` = not yet / must resize again).
    surface_size: [u32; 2],
}

impl<D: HasDisplayHandle, W: HasWindowHandle> SoftwarePresenter<D, W> {
    /// Create a surface for `window` on a shared `context`.
    pub(crate) fn new(context: &softbuffer::Context<D>, window: W) -> Result<Self, RenderError> {
        let surface = softbuffer::Surface::new(context, window).map_err(surface_err)?;
        Ok(Self { surface, _context: None, raster: new_raster(), surface_size: [0, 0] })
    }

    /// Resize (when needed), draw, mask the alpha byte, present. Applies `frame.textures` on
    /// every path, including failures before drawing, but does not clear it.
    fn draw(&mut self, frame: &RenderFrame<'_>) -> Result<(), RenderError> {
        let [w, h] = frame.size_px;
        if self.surface_size != frame.size_px {
            let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else {
                unreachable!("zero size handled by the caller");
            };
            if let Err(e) = self.surface.resize(nw, nh) {
                self.surface_size = [0, 0];
                self.raster.update_textures(frame.textures);
                return Err(surface_err(e));
            }
            self.surface_size = frame.size_px;
        }

        let mut buffer = match self.surface.buffer_mut() {
            Ok(b) => b,
            Err(e) => {
                self.raster.update_textures(frame.textures);
                return Err(surface_err(e));
            }
        };
        let n = w as usize * h as usize;
        if buffer.len() != n {
            self.surface_size = [0, 0];
            self.raster.update_textures(frame.textures);
            return Err(RenderError::Surface(format!("softbuffer returned {} pixels for a {w}x{h} surface", buffer.len())));
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
        buffer.present().map_err(surface_err)
    }
}

impl SoftwarePresenter<RawHandles, RawHandles> {
    /// Host mode: a presenter with its own softbuffer context for a host-owned window.
    ///
    /// # Safety
    /// `handles` must stay valid (the display connection and the window alive) until the
    /// presenter is dropped. The host guarantees this through the `unsafe trait HostWindows`
    /// contract (a window is destroyed only after xdialog released it).
    pub(crate) unsafe fn from_raw(handles: RawHandles) -> Result<Self, RenderError> {
        let context = softbuffer::Context::new(handles).map_err(surface_err)?;
        let mut p = Self::new(&context, handles)?;
        p._context = Some(context);
        Ok(p)
    }
}

impl<D: HasDisplayHandle, W: HasWindowHandle> Presenter for SoftwarePresenter<D, W> {
    fn present(&mut self, frame: RenderFrame<'_>) -> Result<(), RenderError> {
        let result = if is_zero_size(frame.size_px) {
            // Minimised / not yet laid out: nothing to draw, keep the atlas in sync.
            self.raster.update_textures(frame.textures);
            Ok(())
        } else {
            self.draw(&frame)
        };
        frame.textures.clear();
        result
    }
}

/// Bytes `[B, G, R, A]` -> softbuffer words `0x00RRGGBB` (native endian).
pub(super) fn mask_alpha(words: &mut [u32]) {
    for p in words {
        *p = u32::from_le(*p) & 0x00FF_FFFF;
    }
}

fn surface_err(e: softbuffer::SoftBufferError) -> RenderError {
    RenderError::Surface(e.to_string())
}

/// Raw display + window handles of a host-owned window (host mode), usable as softbuffer's
/// `D` and `W`.
///
/// Constructing one is safe; *using* it is only sound while the handles are valid, which is
/// why the only consumer, [`SoftwarePresenter::from_raw`], is `unsafe`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RawHandles {
    pub display: RawDisplayHandle,
    pub window: RawWindowHandle,
}

impl HasDisplayHandle for RawHandles {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // SAFETY: `SoftwarePresenter::from_raw`'s contract: the display outlives the presenter
        // (and so every borrow softbuffer takes through this impl).
        Ok(unsafe { DisplayHandle::borrow_raw(self.display) })
    }
}

impl HasWindowHandle for RawHandles {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: as above, for the window.
        Ok(unsafe { WindowHandle::borrow_raw(self.window) })
    }
}
