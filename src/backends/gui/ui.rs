//! The per-pass API themes build a dialog with: painting primitives (recorded as `draw`
//! [`Shape`]s), text layout ([`TextBlock`]s of the drawing backend's layouts), widget interaction,
//! tweens and a few layout helpers.

use std::hash::{Hash, Hasher};
use std::rc::Rc;

use super::anim::{capsule_pos, Lerp, Transition, Tweens};
use super::clock::Wants;
use super::text::{TextBlock, TextCache, TextStyle};
use super::theme::FrameInfo;
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
    /// right-to-left. Returns where its top-left went.
    pub(crate) fn text_in(&mut self, block: &TextBlock, pos: Point, width: f64, color: Color) -> Point {
        let x = if block.rtl { pos.x + width - block.size.width } else { pos.x };
        let at = Point::new(x, pos.y);
        self.text(block, at, color);
        at
    }

    /// `labels` (the layouts of `texts` in `style`) with every one wider than `max` laid out
    /// again on one line, elided to `max`.
    pub(crate) fn elide_labels(&mut self, labels: Vec<Rc<TextBlock>>, texts: &[String], style: &TextStyle, max: f64) -> Vec<Rc<TextBlock>> {
        labels.into_iter()
              .zip(texts)
              .map(|(l, t)| if l.size.width > max { self.layout(t, style, max, Some(1)) } else { l })
              .collect()
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

    // ---- composite widgets ----------------------------------------------------------------------

    /// The indeterminate "stretchy capsule" (see [`indeterminate_capsule`]) in `track`, restarted
    /// at dialog time `restarted_at`: unsnapped (it glides), round ends; keeps smooth frames
    /// coming.
    pub(crate) fn indeterminate_capsule(&mut self, track: Rect, restarted_at: f64, color: Color) {
        if let Some(c) = indeterminate_capsule(track, (self.time - restarted_at).max(0.0)) {
            self.fill_rect_unsnapped(c, track.height() / 2.0, color);
        }
        self.request_smooth_frame();
    }

    /// A vertically scrolling `viewport` over content `content_h` tall at `*scroll`: applies the
    /// frame's keyboard scroll, and its wheel scroll while the pointer is over the viewport (as a
    /// native scroll view), to this pass's layout; `paint` gets the content's top and is clipped
    /// to the viewport while it scrolls; then the overlay scroll bar `id`, which may drag `*scroll`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn scroll_area(&mut self,
                              id: Id,
                              scroll: &mut f64,
                              viewport: Rect,
                              content_h: f64,
                              frame: &FrameInfo,
                              bar: &ScrollBarSpec,
                              color: Color,
                              paint: impl FnOnce(&mut Self, f64)) {
        let max_scroll = (content_h - viewport.height()).max(0.0);
        let wheel = if self.pointer().is_some_and(|p| viewport.contains(p)) { frame.wheel_request } else { 0.0 };
        *scroll = (*scroll + frame.scroll_request + wheel).clamp(0.0, max_scroll);
        // Clip only while scrolling (a clip layer costs a full composite).
        let clip = max_scroll > 0.0;
        if clip {
            self.push_clip(viewport);
        }
        paint(self, viewport.y0 - *scroll);
        if clip {
            self.pop_clip();
        }
        *scroll = self.overlay_scroll_bar(id, viewport, content_h, *scroll, bar, color);
    }

    /// An overlay scroll bar at the right edge of `viewport` for content `content_h` tall at
    /// `offset`: a thumb that widens while the pointer is over the bar or drags it. Dragging
    /// scrolls; returns the new offset (0 when nothing scrolls).
    pub(crate) fn overlay_scroll_bar(&mut self,
                                     id: Id,
                                     viewport: Rect,
                                     content_h: f64,
                                     offset: f64,
                                     s: &ScrollBarSpec,
                                     color: Color)
                                     -> f64 {
        let (view_h, max) = (viewport.height(), (content_h - viewport.height()).max(0.0));
        if max <= 0.0 {
            return 0.0;
        }
        let x1 = viewport.x1 - s.inset;
        let it = self.interact(id, Rect::new(x1 - s.wide, viewport.y0, x1, viewport.y1));
        let thumb_h = (view_h * view_h / content_h).max(s.min_thumb).min(view_h - 2.0 * s.end_inset);
        let travel = view_h - 2.0 * s.end_inset - thumb_h;
        let mut offset = offset;
        if it.pointer_down && travel > 0.0 {
            offset = (offset + self.pointer_delta().y * max / travel).clamp(0.0, max);
        }
        let w = self.animate(id.with("width"), if it.hovered || it.pointer_down { s.wide as f32 } else { s.thin as f32 }, s.fade) as f64;
        let y = viewport.y0 + s.end_inset + offset / max * travel;
        let radius = s.max_radius.map_or(w / 2.0, |r| r.min(w / 2.0));
        self.fill_rect_unsnapped(Rect::from_origin_size(Point::new(x1 - w, y), Size::new(w, thumb_h)), radius, color);
        offset
    }
}

/// The look of an overlay scroll bar ([`Ui::overlay_scroll_bar`]), logical px.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollBarSpec {
    /// Thumb width at rest, and while hovered or dragged (also the bar's hit width).
    pub thin: f64,
    pub wide: f64,
    /// Gap between the thumb and the viewport's right edge, and its top and bottom.
    pub inset: f64,
    pub end_inset: f64,
    pub min_thumb: f64,
    /// The width change.
    pub fade: Transition,
    /// Thumb corner radius limit (`None`: a pill).
    pub max_radius: Option<f64>,
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

/// Cycle (s) and length (fraction of the free track) of the indeterminate "stretchy capsule"
/// (the Ubuntu theme's timing; the macOS theme shares it).
const CAPSULE_CYCLE: f64 = 3.0;
const CAPSULE_STRETCH: f64 = 0.45;

/// The indeterminate "stretchy capsule" in `track` at `elapsed` seconds into the animation: a
/// capsule `CAPSULE_STRETCH` of the free track long (plus the track's height) that sweeps right
/// over 0-40 % of `CAPSULE_CYCLE`, holds, sweeps back over 50-90 % and holds again (see
/// [`capsule_pos`]), entering and leaving past the track's ends. Clipped to `track`; `None` while
/// nothing of it is inside.
fn indeterminate_capsule(track: Rect, elapsed: f64) -> Option<Rect> {
    let pos = capsule_pos((elapsed.rem_euclid(CAPSULE_CYCLE) / CAPSULE_CYCLE) as f32) as f64;
    let (w, d) = (track.width(), track.height());
    let len = d + CAPSULE_STRETCH * (w - d);
    let cx = (d - len / 2.0) + pos * (w - 2.0 * d + len);
    let (left, right) = ((cx - len / 2.0).max(0.0), (cx + len / 2.0).min(w));
    (right > left).then(|| Rect::new(track.x0 + left, track.y0, track.x0 + right, track.y1))
}
