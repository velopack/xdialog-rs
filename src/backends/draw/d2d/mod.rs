//! The Direct2D + DirectWrite drawing backend (Windows), on the `windows` crate only.
//!
//! - `text.rs`: [`Text`], the per-thread factories (Direct2D, DirectWrite, the system font
//!   collections, fixed rendering parameters), family resolution and [`text::Layout`]s.
//! - `canvas.rs`: [`Canvas`] on an `ID2D1DeviceContext`, and the device-bound resources.
//! - `window.rs`: [`WindowSurface`], an `ID2D1HwndRenderTarget` on the dialog's window.
//! - `wic.rs`: `MemorySurface`, a WIC bitmap (tests and offscreen renders).
//!
//! Live windows need no COM initialisation: the Direct2D and DirectWrite factories are not
//! COM-activated. Only WIC is, and it pins the process MTA.

mod canvas;
mod text;
#[cfg(any(test, feature = "_test-hooks"))]
mod wic;
mod window;

pub(crate) use canvas::Canvas;
pub(crate) use text::Text;
#[cfg(any(test, feature = "_test-hooks"))]
pub(crate) use wic::MemorySurface;
pub(crate) use window::WindowSurface;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

use super::DrawError;

/// Names the golden directory.
pub const NAME: &str = "d2d";

/// Maps a `windows` error to `DrawError::Backend`, naming the call.
fn backend(what: &str) -> impl FnOnce(windows::core::Error) -> DrawError + '_ {
    move |e| DrawError::Backend(format!("d2d: {what}: {e}"))
}

/// Render target properties: BGRA8 (not sRGB: blending happens on the encoded values, like the
/// other backends), 96 DPI until each frame sets its own; `software` forces the software
/// rasterizer (deterministic offscreen renders).
fn target_properties(alpha: D2D1_ALPHA_MODE, software: bool) -> D2D1_RENDER_TARGET_PROPERTIES {
    D2D1_RENDER_TARGET_PROPERTIES { r#type: if software { D2D1_RENDER_TARGET_TYPE_SOFTWARE } else { D2D1_RENDER_TARGET_TYPE_DEFAULT },
                                    pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: alpha },
                                    dpiX: 96.0,
                                    dpiY: 96.0,
                                    usage: D2D1_RENDER_TARGET_USAGE_NONE,
                                    minLevel: D2D1_FEATURE_LEVEL_DEFAULT }
}
