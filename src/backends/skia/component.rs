//! The component model for the skia backend.
//!
//! Each piece of a dialog (background, icon, title/body text, progress bar, buttons) is a
//! [`Component`]. The dialog owns a `Vec<Box<dyn Component>>` in paint/z-order and drives them
//! generically: it measures and positions them by [`Role`], paints the dirty ones onto a single
//! shared pixmap, and dispatches input and controller updates through the trait — with no
//! downcasting and no per-type branching in the dialog.
//!
//! Components store their bounds in **logical** pixels and scale to physical pixels at paint time
//! via [`PaintCtx::scale`], so the layout math is resolution-independent.

use tiny_skia::PixmapMut;

use super::theme::SkiaTheme;

/// Body/label font size in logical pixels.
pub const BODY_SIZE: f32 = 14.0;
/// Title font size in logical pixels.
pub const TITLE_SIZE: f32 = 18.0;
/// Height of the progress bar in logical pixels.
pub const PROGRESS_HEIGHT: f32 = 6.0;

/// A rectangle in logical pixels.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Axis-aligned point containment test (used for hit-testing).
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

/// A measured size in logical pixels.
#[derive(Clone, Copy, Default, Debug)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

/// The layout slot a component occupies. [`super::dialog::SkiaDialog::layout`] positions every
/// component purely from its role, so new content slots into the vertical stack automatically.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// Fills the whole window (painted first).
    Background,
    /// The top-left main icon.
    Icon,
    /// Part of the vertically-stacked content column (title, progress, body).
    Content,
    /// The bottom button strip fill (painted under the buttons).
    Footer,
    /// An interactive button in the bottom row.
    Button,
}

/// Context passed to [`Component::paint`].
pub struct PaintCtx<'a> {
    pub theme: &'a SkiaTheme,
    /// Logical→physical scale factor (HiDPI).
    pub scale: f32,
}

/// Context passed to [`Component::measure`]. Measuring happens entirely in logical pixels
/// (resolution-independent); physical scaling is applied later at paint time.
pub struct LayoutCtx<'a> {
    pub theme: &'a SkiaTheme,
    /// Width available to the component for wrapping, in logical pixels.
    pub available_width: f32,
}

/// A controller-driven update broadcast to every component via [`Component::apply`]. Components
/// that don't care return `false`; this avoids the dialog holding type-specific handles.
pub enum ControllerUpdate<'a> {
    /// Set the determinate progress value (0.0–1.0).
    ProgressValue(f32),
    /// Switch the progress bar to its indeterminate animation.
    ProgressIndeterminate,
    /// Replace the body text.
    BodyText(&'a str),
}

/// A self-contained, self-painting piece of a dialog.
///
/// The dialog never inspects concrete types: layout uses [`Component::role`], painting uses
/// [`Component::is_dirty`]/[`Component::paint`], input uses the interaction methods, and dynamic
/// changes flow through [`Component::apply`]. Default implementations make most components (the
/// static ones) trivial — only buttons and the progress bar override behaviour.
pub trait Component {
    /// The layout slot this component occupies.
    fn role(&self) -> Role;

    /// Measure the component at `ctx.available_width`. May cache an internal layout (hence
    /// `&mut self`). Returns the desired logical size.
    fn measure(&mut self, ctx: &LayoutCtx) -> Size;

    /// Assign the component's logical bounds (called by the dialog's layout pass).
    fn set_bounds(&mut self, b: Rect);

    /// The component's current logical bounds.
    fn bounds(&self) -> Rect;

    /// Whether the component needs repainting.
    fn is_dirty(&self) -> bool;

    /// Paint onto the shared pixmap. The component clears its own bounds first and clears its
    /// dirty flag. `pm` is the whole-window physical pixmap; bounds are scaled by `ctx.scale`.
    fn paint(&mut self, pm: &mut PixmapMut, ctx: &PaintCtx);

    // ── animation (default: static) ─────────────────────────────────────
    /// Advance any animation by `dt` seconds; returns `true` (and self-marks dirty) if it changed.
    fn tick(&mut self, _dt: f32) -> bool {
        false
    }
    /// Whether the component is mid-animation and wants continued frames.
    fn is_animating(&self) -> bool {
        false
    }

    // ── interaction (default: inert; only Button overrides) ─────────────
    /// Whether keyboard focus can land here.
    fn focusable(&self) -> bool {
        false
    }
    fn set_hovered(&mut self, _v: bool) {}
    fn set_pressed(&mut self, _v: bool) {}
    fn set_focused(&mut self, _v: bool) {}
    fn is_hovered(&self) -> bool {
        false
    }
    fn is_pressed(&self) -> bool {
        false
    }
    /// The result/callback index activated by clicking or pressing Enter on this component.
    fn activation_index(&self) -> Option<usize> {
        None
    }

    /// Apply a controller update. Returns `true` if it changed the component's measured size and
    /// therefore requires a relayout.
    fn apply(&mut self, _u: &ControllerUpdate) -> bool {
        false
    }
}
