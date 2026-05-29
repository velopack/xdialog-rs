//! Wrapped text — both the dialog title and the body message, distinguished by a [`LabelKind`]
//! flag rather than two near-identical types.

use tiny_skia::PixmapMut;

use super::component::{Component, ControllerUpdate, LayoutCtx, PaintCtx, Rect, Role, Size, BODY_SIZE, TITLE_SIZE};
use super::renderer::fill_rect;
use super::text::{layout_text, render_text};
use super::theme::SkiaTheme;

/// Which kind of label this is — selects font, size and colour, and whether it reacts to
/// [`ControllerUpdate::BodyText`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LabelKind {
    Title,
    Body,
}

pub struct Label {
    kind: LabelKind,
    text: String,
    bounds: Rect,
    dirty: bool,
}

impl Label {
    pub fn new(kind: LabelKind, text: &str) -> Self {
        Self {
            kind,
            text: text.to_string(),
            bounds: Rect::default(),
            dirty: true,
        }
    }

    fn bold(&self) -> bool {
        matches!(self.kind, LabelKind::Title)
    }

    fn logical_size(&self) -> f32 {
        match self.kind {
            LabelKind::Title => TITLE_SIZE,
            LabelKind::Body => BODY_SIZE,
        }
    }

    fn color(&self, theme: &SkiaTheme) -> (u8, u8, u8) {
        match self.kind {
            LabelKind::Title => theme.color_title_text,
            LabelKind::Body => theme.color_body_text,
        }
    }
}

impl Component for Label {
    fn role(&self) -> Role {
        Role::Content
    }

    fn measure(&mut self, ctx: &LayoutCtx) -> Size {
        // Measure in logical pixels (resolution-independent); paint re-lays-out at physical size.
        let layout = layout_text(&self.text, self.bold(), self.logical_size(), ctx.available_width);
        Size {
            w: layout.total_width,
            h: layout.total_height,
        }
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
        let phys_size = self.logical_size() * s;
        let (x, y, w, h) = (
            self.bounds.x * s,
            self.bounds.y * s,
            self.bounds.w * s,
            self.bounds.h * s,
        );
        // Clear our own bounds to the background, then render the text wrapped at physical width.
        fill_rect(pm, x, y, w, h, ctx.theme.color_background);
        let layout = layout_text(&self.text, self.bold(), phys_size, w);
        render_text(pm, &layout, self.color(ctx.theme), x, y);
        self.dirty = false;
    }

    fn apply(&mut self, u: &ControllerUpdate) -> bool {
        if let ControllerUpdate::BodyText(text) = u {
            if self.kind == LabelKind::Body {
                self.text = text.to_string();
                self.dirty = true;
                return true; // height may have changed → relayout
            }
        }
        false
    }
}
