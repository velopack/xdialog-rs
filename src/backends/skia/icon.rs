//! The main dialog icon (information / warning / error), drawn top-left.

use tiny_skia::{Pixmap, PixmapMut, PixmapPaint, Transform};

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
    /// The icon rasterized once onto a transparent tile at its physical pixel size. The icon's
    /// logical size is fixed, so this is rebuilt only on a DPI change and reused (a cheap
    /// `draw_pixmap` composite) on every relayout repaint.
    tile: Option<(u32, Pixmap)>,
}

impl Icon {
    pub fn new(icon: XDialogIcon) -> Self {
        Self {
            icon,
            bounds: Rect::default(),
            dirty: true,
            tile: None,
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

    fn paint(&mut self, pm: &mut PixmapMut, ctx: &PaintCtx) -> Rect {
        let s = ctx.scale;
        let (x, y, w, h) = (
            self.bounds.x * s,
            self.bounds.y * s,
            self.bounds.w * s,
            self.bounds.h * s,
        );
        // Clear our own bounds to the background, then composite the (cached) icon tile on top.
        fill_rect(pm, x, y, w, h, ctx.theme.color_background);

        let size_px = w.round() as u32;
        if size_px > 0 {
            let stale = self.tile.as_ref().is_none_or(|(sz, _)| *sz != size_px);
            if stale {
                // Rasterize onto a transparent tile; the anti-aliased edges then composite correctly
                // over the background just filled.
                if let Some(mut tile) = Pixmap::new(size_px, size_px) {
                    icons::draw_icon(&mut tile.as_mut(), &self.icon, 0.0, 0.0, size_px as f32);
                    self.tile = Some((size_px, tile));
                }
            }
            if let Some((_, tile)) = self.tile.as_ref() {
                pm.draw_pixmap(
                    x.round() as i32,
                    y.round() as i32,
                    tile.as_ref(),
                    &PixmapPaint::default(),
                    Transform::identity(),
                    None,
                );
            }
        }

        self.dirty = false;
        Rect::new(x, y, w, h)
    }
}
