use std::sync::LazyLock;

use mina::prelude::*;
use tiny_skia::PixmapMut;

use super::component::{
    Component, ControllerUpdate, LayoutCtx, PaintCtx, Rect, Role, Size, PROGRESS_HEIGHT,
};
use super::renderer::{fill_rect, fill_rounded_rect};

#[derive(Animate, Clone, Debug, Default, PartialEq)]
pub struct ProgressState {
    pub x1: f32,
    pub x2: f32,
}

/// Normalized travel position of the indeterminate capsule: 0 = parked (as a circle) at the left,
/// 1 = parked (as a circle) at the right. mina drives this; the circle→pill stretch is derived from
/// it analytically at paint time so the ends are always exact circles.
#[derive(Animate, Clone, Debug, Default, PartialEq)]
struct PillPos {
    pos: f32,
}

/// Indeterminate cycle length, seconds. Keep in sync with the `timeline!` duration below.
const INDETERMINATE_CYCLE: f32 = 2.20;
/// Constant capsule length during travel, as a fraction of the track beyond the circle diameter.
const INDETERMINATE_STRETCH: f32 = 0.45;

// macOS-style "stretchy capsule": the capsule eases out from a circle at one end, elongating into a
// pill as it speeds up, then decelerates and contracts back to a perfect circle at the far end,
// pauses briefly, and reverses. mina drives the position with an ease-in-out; the two flat keyframe
// segments are the end pauses. The circle↔pill stretch is computed in `paint` from this position.
static INDETERMINATE_TIMELINE: LazyLock<PillPosTimeline> = LazyLock::new(|| {
    timeline!(
        PillPos 2.20s Easing::InOutCubic
        from { pos: 0.0 } // circle at the left, about to leave
        40%  { pos: 1.0 } // sweep across, arriving as a circle at the right
        50%  { pos: 1.0 } // brief pause at the right
        90%  { pos: 0.0 } // sweep back, arriving as a circle at the left
        to   { pos: 0.0 } // brief pause at the left → seamless loop
    )
});

pub struct SkiaProgressBar {
    pub state: ProgressState,
    pub is_indeterminate: bool,
    current_time: f32,
    /// Indeterminate capsule position; only meaningful while `is_indeterminate`.
    pill: PillPos,
    value_animator: Option<ProgressStateTimeline>,
    bounds: Rect,
    dirty: bool,
}

impl SkiaProgressBar {
    pub fn new() -> Self {
        Self {
            state: ProgressState::default(),
            is_indeterminate: false,
            current_time: 0.0,
            pill: PillPos::default(),
            value_animator: None,
            bounds: Rect::default(),
            dirty: true,
        }
    }

    fn set_value(&mut self, value: f32) {
        let animation: ProgressStateTimeline = timeline!(
            ProgressState 0.3s Easing::OutCubic
            from { x1: 0.0, x2: self.state.x2 }
            to { x1: 0.0, x2: value }
        );

        self.is_indeterminate = false;
        self.current_time = 0.0;
        self.value_animator = Some(animation);
    }

    fn set_indeterminate(&mut self) {
        self.is_indeterminate = true;
        self.current_time = 0.0;
        self.pill = PillPos::default();
    }

    /// Advance the timeline by `elapsed_secs`; returns whether the visible state changed.
    fn advance(&mut self, elapsed_secs: f32) -> bool {
        if self.is_indeterminate {
            let before = self.pill.pos;
            INDETERMINATE_TIMELINE.update(&mut self.pill, self.current_time);
            self.current_time += elapsed_secs;
            if self.current_time > INDETERMINATE_CYCLE {
                self.current_time = 0.0;
            }
            self.pill.pos != before
        } else if let Some(ref mut animator) = self.value_animator {
            let before = self.state.clone();
            // Advance first, then sample: this guarantees the final tick samples at (or past) the
            // 0.3s end of the timeline so the bar always lands exactly on the target value, even if
            // a single tick covers the whole animation.
            self.current_time += elapsed_secs;
            animator.update(&mut self.state, self.current_time);
            if self.current_time >= 0.3 {
                self.value_animator = None;
            }
            self.state != before
        } else {
            false
        }
    }
}

impl Component for SkiaProgressBar {
    fn role(&self) -> Role {
        Role::Content
    }

    fn measure(&mut self, _ctx: &LayoutCtx) -> Size {
        // No intrinsic width (it stretches to the content column); fixed logical height.
        Size {
            w: 0.0,
            h: PROGRESS_HEIGHT,
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

    fn paint(&mut self, pm: &mut PixmapMut, ctx: &PaintCtx) -> Rect {
        let s = ctx.scale;
        let (x, y, w, h) = (
            self.bounds.x * s,
            self.bounds.y * s,
            self.bounds.w * s,
            self.bounds.h * s,
        );
        // Clear our own bounds to the background.
        fill_rect(pm, x, y, w, h, ctx.theme.color_background);

        if self.is_indeterminate {
            // Fully-rounded "pill" track and a stretchy capsule. A circle has diameter == bar
            // height; passing radius = h/2 yields a circle when width == h and a stadium when wider.
            let r = h / 2.0;
            fill_rounded_rect(pm, x, y, w, h, r, ctx.theme.color_progress_background);

            let pos = self.pill.pos.clamp(0.0, 1.0);
            let d = h; // circle diameter (== bar height)
            // A constant-width capsule whose centre sweeps from just outside the left wall to just
            // outside the right. Clamping each edge to the track makes it expand from a circle while
            // anchored to the near wall, travel at constant width, then contract back to a circle
            // anchored to the far wall — the macOS behaviour.
            let len = d + INDETERMINATE_STRETCH * (w - d);
            let cx = (d - len / 2.0) + pos * (w - 2.0 * d + len);
            let left = (cx - len / 2.0).max(0.0);
            let right = (cx + len / 2.0).min(w);
            fill_rounded_rect(pm, x + left, y, right - left, h, r, ctx.theme.color_progress_foreground);
        } else {
            let radius = 2.0 * s;
            // Background track.
            fill_rounded_rect(pm, x, y, w, h, radius, ctx.theme.color_progress_background);

            // Foreground bar. Clamp to [0, 1] as a render-layer guard so the bar can never draw past
            // the track regardless of how the state was set (the public API also clamps the input).
            let bar_start = self.state.x1.clamp(0.0, 1.0) * w;
            let bar_end = self.state.x2.clamp(0.0, 1.0) * w;
            let bar_w = bar_end - bar_start;
            if bar_w > 0.0 {
                fill_rounded_rect(
                    pm,
                    x + bar_start,
                    y,
                    bar_w,
                    h,
                    radius,
                    ctx.theme.color_progress_foreground,
                );
            }
        }

        self.dirty = false;
        Rect::new(x, y, w, h)
    }

    fn tick(&mut self, dt: f32) -> bool {
        let changed = self.advance(dt);
        if changed {
            self.dirty = true;
        }
        changed
    }

    fn is_animating(&self) -> bool {
        self.is_indeterminate || self.value_animator.is_some()
    }

    fn apply(&mut self, u: &ControllerUpdate) -> bool {
        match u {
            ControllerUpdate::ProgressValue(v) => {
                self.set_value(*v);
                self.dirty = true;
            }
            ControllerUpdate::ProgressIndeterminate => {
                self.set_indeterminate();
                self.dirty = true;
            }
            ControllerUpdate::BodyText(_) => {}
        }
        false // progress changes never alter layout
    }
}
