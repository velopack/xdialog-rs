//! The per-pass API themes build a dialog with: painting primitives (recorded as `draw`
//! [`Shape`]s), text layout ([`TextBlock`]s of the drawing backend's layouts), widget interaction,
//! tweens and a few layout helpers.

use std::hash::{Hash, Hasher};
use std::rc::Rc;

use super::anim::{Lerp, Transition, Tweens};
use super::clock::Wants;
use super::text::{TextBlock, TextCache, TextStyle};
use crate::backends::draw::{Color, Image, LineCap, Point, Rect, Shape, Size, Text, Vec2};

/// A stable widget identity (tweens, hit testing, focus).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Id(u64);

impl Id {
    pub(crate) fn new(name: &str) -> Id {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        name.hash(&mut h);
        Id(h.finish())
    }

    /// A child id (`id.with(index)`, `id.with("part")`).
    pub(crate) fn with(self, salt: impl Hash) -> Id {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.0, salt).hash(&mut h);
        Id(h.finish())
    }
}

/// Input and widget state that outlives a pass (one per dialog). Core updates the pointer, press
/// and focus fields from input before a pass; the pass reads them.
pub(crate) struct UiState {
    /// Pointer position (`None`: outside the window).
    pub pointer: Option<Point>,
    /// The widget a primary press started on, while it is held.
    pub pressed: Option<Id>,
    /// API index of the focused button.
    pub focus: Option<usize>,
    pub window_focused: bool,
    /// The pointer moved / left the window since the last pass.
    pub moved: bool,
    pub gone: bool,
    /// Pointer position at the last pass (drag deltas).
    pub last_pointer: Option<Point>,
    pub(crate) tweens: Tweens,
    pub(crate) texts: TextCache,
}

impl UiState {
    /// Widget state laying out text with `text`.
    pub(crate) fn new(text: Rc<Text>) -> Self {
        UiState { pointer: None,
                  pressed: None,
                  focus: None,
                  window_focused: true,
                  moved: false,
                  gone: false,
                  last_pointer: None,
                  tweens: Tweens::default(),
                  texts: TextCache::new(text) }
    }

    /// End of a pass: input flags consumed.
    pub(crate) fn end_pass(&mut self) {
        self.moved = false;
        self.gone = false;
        self.last_pointer = self.pointer;
        self.texts.end_pass();
    }
}

/// What the pointer does with a widget this pass.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Interaction {
    /// The pointer is inside the rect.
    pub contains_pointer: bool,
    /// Inside, and no other widget is being pressed.
    pub hovered: bool,
    /// A primary press started on this widget and is still held.
    pub pointer_down: bool,
}

/// One pass: the theme's view of core (see the module docs).
pub(crate) struct Ui<'a> {
    st: &'a mut UiState,
    time: f64,
    pub(crate) shapes: Vec<Shape>,
    /// Interactive rects in paint order (core hit-tests the next input against them).
    pub(crate) hits: Vec<(Id, Rect)>,
    /// A tween is running: another frame at the cadence.
    pub(crate) repaint: bool,
    /// Continuous motion: frames at the monitor's refresh rate.
    pub(crate) smooth: bool,
}

impl<'a> Ui<'a> {
    /// A pass at dialog time `time` (call `st.texts.begin_pass` first).
    #[cfg(all(test, draw_soft))]
    pub(crate) fn new(st: &'a mut UiState, time: f64) -> Self {
        Self::with_buffers(st, time, Vec::new(), Vec::new())
    }

    /// A pass recording into `shapes` and `hits` (cleared first: buffers reused across frames).
    pub(crate) fn with_buffers(st: &'a mut UiState, time: f64, mut shapes: Vec<Shape>, mut hits: Vec<(Id, Rect)>) -> Self {
        shapes.clear();
        hits.clear();
        Ui { st, time, shapes, hits, repaint: false, smooth: false }
    }

    /// End the pass: the drawing, the interactive rects and what the next frame should be.
    pub(crate) fn finish(self) -> (Vec<Shape>, Vec<(Id, Rect)>, Wants) {
        (self.shapes, self.hits, Wants { repaint: self.repaint, smooth: self.smooth })
    }

    // ---- time and animation ---------------------------------------------------------------------

    /// Dialog-clock seconds.
    pub(crate) fn time(&self) -> f64 {
        self.time
    }

    /// Interruptible tween of `id` towards `target` (see [`Tweens::animate`]); keeps frames coming
    /// while it runs.
    pub(crate) fn animate<T: Lerp>(&mut self, id: Id, target: T, tr: Transition) -> T {
        let (value, active) = self.st.tweens.animate(id, self.time, target, tr);
        self.repaint |= active;
        value
    }

    /// Freeze the tween of `id` at its displayed value.
    pub(crate) fn stop_animation<T: Lerp>(&mut self, id: Id) {
        self.st.tweens.stop::<T>(id, self.time);
    }

    /// Continuous motion (an indeterminate progress bar): frames at the monitor's refresh rate
    /// while the theme keeps asking.
    pub(crate) fn request_smooth_frame(&mut self) {
        self.repaint = true;
        self.smooth = true;
    }

    // ---- input ----------------------------------------------------------------------------------

    /// Register an interactive rect for this pass and report what the pointer does with it.
    pub(crate) fn interact(&mut self, id: Id, rect: Rect) -> Interaction {
        self.hits.push((id, rect));
        let contains = self.st.pointer.is_some_and(|p| rect.contains(p));
        let down = self.st.pressed == Some(id);
        Interaction { contains_pointer: contains, hovered: contains && (self.st.pressed.is_none() || down), pointer_down: down }
    }

    /// Pointer movement since the last pass (zero when outside).
    pub(crate) fn pointer_delta(&self) -> Vec2 {
        match (self.st.pointer, self.st.last_pointer) {
            (Some(p), Some(l)) => p - l,
            _ => Vec2::ZERO,
        }
    }

    pub(crate) fn pointer(&self) -> Option<Point> {
        self.st.pointer
    }

    /// The pointer moved / left the window since the last pass.
    pub(crate) fn pointer_moved(&self) -> bool {
        self.st.moved
    }

    pub(crate) fn pointer_gone(&self) -> bool {
        self.st.gone
    }

    pub(crate) fn focused_button(&self) -> Option<usize> {
        self.st.focus
    }

    pub(crate) fn window_focused(&self) -> bool {
        self.st.window_focused
    }

    // ---- text -----------------------------------------------------------------------------------

    /// Lay out `text` in the theme's fonts (cached): wrapped at `wrap_width` (`f64::INFINITY` =
    /// no wrapping), at most `max_lines` lines (elided).
    pub(crate) fn layout(&mut self, text: &str, style: &TextStyle, wrap_width: f64, max_lines: Option<usize>) -> Rc<TextBlock> {
        self.st.texts.layout(text, style, wrap_width, max_lines)
    }

    /// Paint `block` with its top-left at `pos` (one shape per paragraph).
    pub(crate) fn text(&mut self, block: &TextBlock, pos: Point, color: Color) {
        self.shapes.extend(block.paras.iter().map(|(layout, at)| Shape::Text { layout: layout.clone(), pos: pos + at.to_vec2(), color }));
    }

    /// Paint `block` in a column of `width` starting at `pos`: right-aligned when it starts
    /// right-to-left.
    pub(crate) fn text_in(&mut self, block: &TextBlock, pos: Point, width: f64, color: Color) {
        let x = if block.rtl { pos.x + width - block.size.width } else { pos.x };
        self.text(block, Point::new(x, pos.y), color);
    }

    // ---- painting -------------------------------------------------------------------------------

    /// A filled (rounded) rect, edges snapped to physical pixels.
    pub(crate) fn fill_rect(&mut self, rect: Rect, radius: f64, color: Color) {
        self.shapes.push(Shape::Rect { rect, radius, color, snap: true });
    }

    /// A filled (rounded) rect at its exact position (a moving end glides instead of stepping).
    pub(crate) fn fill_rect_unsnapped(&mut self, rect: Rect, radius: f64, color: Color) {
        self.shapes.push(Shape::Rect { rect, radius, color, snap: false });
    }

    /// A (rounded) rect filled with a vertical gradient from `top` to `bottom`, edges snapped.
    pub(crate) fn fill_rect_gradient(&mut self, rect: Rect, radius: f64, top: Color, bottom: Color) {
        self.shapes.push(Shape::Gradient { rect, radius, top, bottom });
    }

    /// A filled closed polygon.
    pub(crate) fn polygon(&mut self, points: &[Point], color: Color) {
        self.shapes.push(Shape::Polygon { points: points.into(), color });
    }

    /// A stroke of `width` centred on the (rounded) rect's edge.
    pub(crate) fn stroke_rect(&mut self, rect: Rect, radius: f64, width: f64, color: Color) {
        self.shapes.push(Shape::Stroke { rect, radius, width, color });
    }

    pub(crate) fn circle(&mut self, center: Point, radius: f64, color: Color) {
        self.shapes.push(Shape::Circle { center, radius, color });
    }

    pub(crate) fn line(&mut self, from: Point, to: Point, width: f64, cap: LineCap, color: Color) {
        self.shapes.push(Shape::Line { from, to, width, cap, color });
    }

    /// `image` stretched over `rect` (the custom icon: `rect` × the scale is the image size).
    pub(crate) fn image(&mut self, image: &Image, rect: Rect) {
        self.shapes.push(Shape::Image { image: image.clone(), rect });
    }

    /// Clip what is painted until [`Ui::pop_clip`] to `rect`.
    pub(crate) fn push_clip(&mut self, rect: Rect) {
        self.shapes.push(Shape::PushClip(rect));
    }

    pub(crate) fn pop_clip(&mut self) {
        self.shapes.push(Shape::PopClip);
    }
}

// ---- layout helpers -----------------------------------------------------------------------------

/// A top-to-bottom stack of rows spanning `x0..x1`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Column {
    pub x0: f64,
    pub x1: f64,
    /// Top of the next row.
    pub y: f64,
}

impl Column {
    pub(crate) fn new(x0: f64, x1: f64, y: f64) -> Self {
        Column { x0, x1, y }
    }

    /// The next row, `height` tall.
    pub(crate) fn row(&mut self, height: f64) -> Rect {
        let r = Rect::new(self.x0, self.y, self.x1, self.y + height);
        self.y += height;
        r
    }

    pub(crate) fn space(&mut self, gap: f64) {
        self.y += gap;
    }
}

/// `n` equal columns of `rect`, `gap` apart, left to right.
pub(crate) fn columns(rect: Rect, n: usize, gap: f64) -> Vec<Rect> {
    let w = (rect.width() - gap * n.saturating_sub(1) as f64) / n.max(1) as f64;
    (0..n).map(|i| {
              let x = rect.x0 + i as f64 * (w + gap);
              Rect::new(x, rect.y0, x + w, rect.y1)
          })
          .collect()
}

/// A `size` rect centred in `outer`.
pub(crate) fn centered(outer: Rect, size: Size) -> Rect {
    Rect::from_center_size(outer.center(), size)
}

/// Top-left of a one-line `label` centred horizontally in `outer`, with its capitals centred
/// vertically (see `TextBlock::cap_center`).
pub(crate) fn caps_centered(outer: Rect, label: &TextBlock) -> Point {
    Point::new(centered(outer, label.size).x0, outer.center().y - label.cap_center())
}
