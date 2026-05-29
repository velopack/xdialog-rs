//! The main dialog icon (information / warning / error), drawn top-left.

use tiny_skia::PixmapMut;

use crate::model::XDialogIcon;

use super::component::{Component, LayoutCtx, PaintCtx, Rect, Role, Size};
use super::icons;
use super::renderer::fill_rect;

/// Wraps [`icons::draw_icon`] as a component. Measures to the theme's `main_icon_size` and is
/// static (only repaints on resize/relayout).
pub struct Icon {
    icon: XDialogIcon,
    bounds: Rect,
    dirty: bool,
}

impl Icon {
    pub fn new(icon: XDialogIcon) -> Self {
        Self {
            icon,
            bounds: Rect::default(),
            dirty: true,
        }
    }
}

impl Component for Icon {
    fn role(&self) -> Role {
        Role::Icon
    }

    fn measure(&mut self, ctx: &LayoutCtx) -> Size {
        let s = ctx.theme.main_icon_size as f32;
        Size { w: s, h: s }
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
        let s = ctx.scale;
        let (x, y, w, h) = (
            self.bounds.x * s,
            self.bounds.y * s,
            self.bounds.w * s,
            self.bounds.h * s,
        );
        // Clear our own bounds to the background before drawing.
        fill_rect(pm, x, y, w, h, ctx.theme.color_background);
        icons::draw_icon(pm, &self.icon, x, y, w);
        self.dirty = false;
    }
}
