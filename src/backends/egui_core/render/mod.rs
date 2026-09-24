//! Presenters. Software only; the trait keeps the door open for a GPU presenter later.
//!
//! - `raster/`: the vendored `egui_software_backend` rasterizer (MIT OR Apache-2.0, see its
//!   `mod.rs` and `LICENSE-*` files).
//! - `software.rs`: `SoftwarePresenter` (softbuffer) and `MemoryPresenter` (offscreen RGBA).

use egui::Color32;

mod raster;
mod software;
#[cfg(test)]
mod tests;

// Consumed by own_loop.rs / host.rs / offscreen.rs.
#[allow(unused_imports)]
pub(crate) use software::{MemoryPresenter, RawHandles, SoftwarePresenter};

/// One frame to present.
pub(crate) struct RenderFrame<'a> {
    pub prims: &'a [egui::ClippedPrimitive],
    /// The presenter applies these texture changes, then clears them.
    pub textures: &'a mut egui::TexturesDelta,
    pub size_px: [u32; 2],
    pub ppp: f32,
    /// Buffer clear colour (the theme style's `visuals.panel_fill`).
    pub clear: Color32,
}

#[derive(Debug)]
pub(crate) enum RenderError {
    /// The surface was lost or could not be resized/presented; core recreates the presenter.
    Surface(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::Surface(s) => write!(f, "surface error: {s}"),
        }
    }
}

pub(crate) trait Presenter {
    /// Present one frame. Must `frame.textures.clear()` after applying it. A no-op while
    /// `size_px` has a zero side (the delta is still applied and cleared).
    fn present(&mut self, frame: RenderFrame<'_>) -> Result<(), RenderError>;
    /// Offscreen/test only: the last frame as RGBA8 `(width, height, pixels)`.
    fn read_rgba(&self) -> Option<(u32, u32, &[u8])> {
        None
    }
}
