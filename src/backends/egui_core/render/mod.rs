//! Presenters: `SoftwarePresenter` (windows) and `MemoryPresenter` (offscreen and tests).
//!
//! - `raster/`: the vendored `egui_software_backend` rasterizer (MIT OR Apache-2.0, see its
//!   `mod.rs` and `LICENSE-*` files).
//! - `software.rs`: `SoftwarePresenter` (softbuffer) and `MemoryPresenter` (offscreen RGBA).

use egui::Color32;

mod raster;
mod software;
#[cfg(test)]
mod tests;

pub(crate) use software::SoftwarePresenter;
#[cfg(any(test, feature = "_test-hooks"))]
pub(crate) use software::MemoryPresenter;

/// One frame to present.
pub(crate) struct RenderFrame<'a> {
    pub prims: &'a [egui::ClippedPrimitive],
    /// Texture changes to apply first (the caller clears them afterwards).
    pub textures: &'a egui::TexturesDelta,
    pub size_px: [u32; 2],
    pub ppp: f32,
    /// Buffer clear colour (the theme style's `visuals.panel_fill`).
    pub clear: Color32,
}

pub(crate) trait Presenter {
    /// Present one frame; a no-op while `size_px` has a zero side (the texture changes are still
    /// applied). Errors are for the log.
    fn present(&mut self, frame: RenderFrame<'_>) -> Result<(), String>;
    /// Offscreen/test only: the last frame as RGBA8 `(width, height, pixels)`.
    #[cfg(any(test, feature = "_test-hooks"))]
    fn read_rgba(&self) -> Option<(u32, u32, &[u8])> {
        None
    }
}
