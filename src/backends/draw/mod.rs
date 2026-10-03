//! xdialog's drawing layer: a display list ([`Shape`]) recorded by the themes in logical px, one
//! shared [`list::replay`] that snaps it to physical pixels, and exactly one backend per target
//! that draws it and lays out text:
//!
//! | target | backend | technology |
//! |---|---|---|
//! | Windows | `d2d` | Direct2D + DirectWrite |
//! | macOS | `cg` | CoreGraphics + CoreText |
//! | everything else | `soft` | vello_cpu + cosmic-text (software) |
//!
//! The contract every backend implements is the traits below; the rest of the crate only uses the
//! concrete aliases ([`Text`], [`Family`], [`Layout`], [`WindowSurface`], `MemorySurface`).

pub(crate) mod color;
pub(crate) mod geom;
pub(crate) mod image;
pub(crate) mod list;
#[cfg(not(windows))]
pub(crate) mod softbuffer_surface;
#[cfg(test)]
mod tests;

#[cfg(windows)]
#[path = "d2d/mod.rs"]
mod backend;
#[cfg(target_os = "macos")]
#[path = "cg/mod.rs"]
mod backend;
#[cfg(not(any(windows, target_os = "macos")))]
#[path = "soft/mod.rs"]
mod backend;

use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) use color::Color;
pub(crate) use geom::*;
pub(crate) use image::Image;
pub(crate) use list::{Frame, LineCap, Shape};

/// Start the background system font scan (idempotent); called early so fallback fonts are
/// usually ready before a dialog needs them.
#[cfg(draw_soft)]
pub(crate) use backend::fonts::start_scan as start_font_scan;

/// CSS weight scale: 400 regular, 600 semibold, 700 bold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Weight(pub u16);

impl Weight {
    pub const REGULAR: Weight = Weight(400);
    pub const SEMIBOLD: Weight = Weight(600);
    pub const BOLD: Weight = Weight(700);
}

/// One paragraph's layout parameters. Deliberately no colour.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextParams<'a> {
    /// Resolved once per theme-font change.
    pub family: &'a Family,
    /// Logical px.
    pub size: f64,
    pub weight: Weight,
    /// Variable-font optical size in pt (Fluent: px*0.75). D2D applies it via IDWriteFactory6
    /// axis values; CG and soft ignore it. None = font default.
    #[cfg_attr(not(windows), allow(dead_code))] // D2D only
    pub optical_size: Option<f64>,
    /// Uniform line pitch in logical px (Fluent size*2724/2048, Ubuntu size*1.2); None = font's
    /// natural. The surplus over ascent + descent is split evenly above and below (CSS half-leading),
    /// so text centred by its layout box is centred visually.
    pub line_height: Option<f64>,
    /// None = no wrapping. Some(w) = wrap at whitespace, emergency-break over-long words.
    pub max_width: Option<f64>,
    /// Paragraph base direction (from unicode_bidi::get_base_direction); true = lines right-aligned
    /// within max_width.
    pub rtl: bool,
    /// Centre every line within `max_width` (unwrapped: within the widest line) instead of
    /// start-aligning it.
    pub center: bool,
}

/// Per-thread text/font service (not Send) from `Text::shared()`, used by the runtime, offscreen
/// rendering and tests alike. `&self` everywhere: backends cache behind interior mutability.
pub(crate) trait TextSystem: Sized + 'static {
    type Family: Clone + PartialEq + core::fmt::Debug;
    type Layout: TextLayout;
    /// Thread-local instance, created on first use (warm D2D/DWrite factories: ~160 ms vs ~10 ms
    /// first emoji frame, spike f7fa4b8). Err = backend unavailable (`Auto` falls back to Win32).
    fn shared() -> Result<std::rc::Rc<Self>, DrawError>;
    /// First of `candidates` the platform has, else the platform UI font
    /// (Segoe UI / SF via CTFontCreateUIFontForLanguage / bundled Ubuntu).
    fn resolve_family(&self, candidates: &[&str]) -> Self::Family;
    fn layout(&self, text: &str, p: &TextParams<'_>) -> Self::Layout;
    /// soft only: make fallback faces covering `texts` available, waiting up to `wait`;
    /// false = still loading (runtime is woken via runtime::wake_all when the scan merges).
    fn prepare_fonts(&self, _family: &Self::Family, _texts: &[&str], _wait: std::time::Duration) -> bool {
        true
    }
    /// Bumped when available fonts change (all layouts stale). Native backends: always 0.
    fn generation(&self) -> u64 {
        0
    }
}

/// A shaped, wrapped paragraph. Cheap Clone (Rc), colour-free, immutable.
pub(crate) trait TextLayout: Clone + 'static {
    /// Widest line EXCLUDING trailing whitespace × total height (logical px).
    fn size(&self) -> Size;
    fn line_count(&self) -> usize;
    /// The first line's baseline below the layout's top (logical px).
    fn first_baseline(&self) -> f64;
    /// Cap height of the paragraph's primary font (logical px): with `first_baseline`, what
    /// centres a label's capitals optically.
    fn cap_height(&self) -> f64;
    /// Process-unique monotonic id assigned at creation (NOT a pointer — no ABA). Display lists
    /// compare text by it (`Shape::eq`); the soft backend's text rasters key on (id, scale).
    fn id(&self) -> u64;
}

/// Immediate drawing in LOGICAL px; the canvas applies ppp itself (D2D SetDpi, CG scale CTM,
/// vello root affine). Only `list::replay` calls it, always with already-snapped geometry;
/// implementations never snap.
pub(crate) trait Canvas {
    fn clear(&mut self, color: Color);
    /// Anti-aliased fill; radius 0 = plain rect (fast path).
    fn fill_rect(&mut self, rect: Rect, radius: f64, color: Color);
    /// Stroke centred on the edge of `rect`; callers inflate for inside/outside.
    fn stroke_rect(&mut self, rect: Rect, radius: f64, width: f64, color: Color);
    fn fill_circle(&mut self, center: Point, radius: f64, color: Color);
    fn line(&mut self, from: Point, to: Point, width: f64, cap: LineCap, color: Color);
    /// Anti-aliased (rounded) rect fill, vertical linear gradient from `top` to `bottom`.
    fn fill_rect_gradient(&mut self, rect: Rect, radius: f64, top: Color, bottom: Color);
    /// Anti-aliased fill of the closed polygon through `points` (non-zero winding).
    fn fill_polygon(&mut self, points: &[Point], color: Color);
    /// Axis-aligned, aliased, intersected with the current clip. Balanced by pop_clip.
    fn push_clip(&mut self, rect: Rect);
    fn pop_clip(&mut self);
    fn draw_text(&mut self, layout: &Layout, top_left: Point, color: Color);
    /// Bilinear. For the icon, dst's physical size == image.size() (1 texel : 1 px).
    fn draw_image(&mut self, image: &Image, dst: Rect);
}

/// Where frames go.
pub(crate) trait Surface {
    /// A no-op for a zero-sized frame (minimised, not laid out yet): a memory surface keeps its
    /// previous image.
    fn present(&mut self, frame: &Frame<'_>) -> Result<(), DrawError> {
        if list::is_zero_size(frame.size_px) {
            return Ok(());
        }
        self.present_nonzero(frame)
    }

    /// `present` for a frame with both sides non-zero.
    fn present_nonzero(&mut self, frame: &Frame<'_>) -> Result<(), DrawError>;
}

pub(crate) trait WindowTarget: Surface + Sized {
    /// Created after the window exists and before it is shown. A device target that fails to
    /// present is recreated once inside `present`.
    fn new(window: &std::rc::Rc<winit::window::Window>, text: &std::rc::Rc<Text>) -> Result<Self, DrawError>;
}

#[cfg(any(test, feature = "_test-hooks"))]
pub(crate) trait MemoryTarget: Surface + Sized {
    fn new(text: &std::rc::Rc<Text>) -> Result<Self, DrawError>;
    /// Last frame as opaque straight RGBA8 (alpha 255), row-major, tightly packed.
    fn read_rgba(&self) -> Option<(u32, u32, &[u8])>;
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DrawError {
    #[error("{0}")]
    Backend(String),
}

pub(crate) type Text = backend::Text;
pub(crate) type Family = <Text as TextSystem>::Family;
pub(crate) type Layout = <Text as TextSystem>::Layout;
pub(crate) type WindowSurface = backend::WindowSurface;
#[cfg(any(test, feature = "_test-hooks"))]
pub(crate) type MemorySurface = backend::MemorySurface;
/// "soft" | "d2d" | "cg" — names the golden directory; re-exported in __test.
#[cfg_attr(not(any(test, feature = "_test-hooks")), allow(dead_code))]
pub const RENDERER: &str = backend::NAME;

/// A process-unique, monotonic id (images, text layouts): what backend caches key on.
pub(crate) fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

// Compile-time conformance: each backend must implement exactly this contract. Caught by
// `cargo check --target …` for every OS from any host.
const _: () = {
    fn text<T: TextSystem>() {}
    fn canvas<C: Canvas>() {}
    fn window<W: WindowTarget>() {}
    #[allow(dead_code)]
    fn check() {
        text::<backend::Text>();
        canvas::<backend::Canvas<'static>>();
        window::<backend::WindowSurface>();
        #[cfg(any(test, feature = "_test-hooks"))]
        {
            fn mem<M: MemoryTarget>() {}
            mem::<backend::MemorySurface>();
        }
    }
};
