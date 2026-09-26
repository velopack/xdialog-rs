//! The CoreGraphics + CoreText drawing backend (macOS).
//!
//! - `text.rs`: [`Text`] (fonts, CTTypesetter line breaking; bidi, the font cascade and colour
//!   emoji are CoreText's).
//! - `canvas.rs`: [`Canvas`] on a `CGContext` (y flipped to point down, scaled by `ppp`).
//! - `window.rs`: [`WindowSurface`] (a CG bitmap context straight over softbuffer's buffer) and
//!   `MemorySurface` (the same over an owned buffer).
//!
//! Everything is drawn in device RGB (no colour management, so the premultiplied sRGB-encoded
//! colours land in the pixels unchanged and blending is gamma-space, like the other backends).

mod canvas;
mod text;
mod window;

pub(crate) use canvas::Canvas;
pub(crate) use text::Text;
#[cfg(any(test, feature = "_test-hooks"))]
pub(crate) use window::MemorySurface;
pub(crate) use window::WindowSurface;

/// Names the golden directory.
pub const NAME: &str = "cg";
