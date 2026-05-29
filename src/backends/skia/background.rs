//! Static fill components: the window background and the bottom button strip.

use tiny_skia::PixmapMut;

use super::component::{Component, LayoutCtx, PaintCtx, Rect, Role, Size};
use super::renderer::fill_rect;

/// Fills the entire window with `color_background`. Painted first, so every other component draws
/// on top of it. Only repaints on resize/relayout.
pub struct Background {
    bounds: Rect,
    dirty: bool,
}

impl Background {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            dirty: true,
        }
    }
}

impl Component for Background {
    fn role(&self) -> Role {
        Role::Background
    }

    fn measure(&mut self, _ctx: &LayoutCtx) -> Size {
        Size::default()
    }

    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn paint(&mut self, pm: &mut PixmapMut, ctx: &PaintCtx) {
        // Fill the entire physical pixmap (not the logical-derived bounds) so sub-pixel rounding of
        // the window size never leaves an uninitialised edge sliver.
        let (w, h) = (pm.width() as f32, pm.height() as f32);
        fill_rect(pm, 0.0, 0.0, w, h, ctx.theme.color_background);
        self.dirty = false;
    }
}

/// Fills the bottom button strip with `color_background_alt`. Painted before the buttons (which sit
/// on top and clear themselves to the same alt colour), so per-button hover repaints don't need to
/// touch the strip.
pub struct Footer {
    bounds: Rect,
    dirty: bool,
}

impl Footer {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            dirty: true,
        }
    }
}

impl Component for Footer {
    fn role(&self) -> Role {
        Role::Footer
    }

    fn measure(&mut self, _ctx: &LayoutCtx) -> Size {
        Size::default()
    }

    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn paint(&mut self, pm: &mut PixmapMut, ctx: &PaintCtx) {
        // Span the full physical width; height/position come from the (logical) bounds.
        let s = ctx.scale;
        let w = pm.width() as f32;
        fill_rect(pm, 0.0, self.bounds.y * s, w, self.bounds.h * s, ctx.theme.color_background_alt);
        self.dirty = false;
    }
}
