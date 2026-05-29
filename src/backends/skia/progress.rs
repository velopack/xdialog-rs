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

static INDETERMINATE_TIMELINE: LazyLock<ProgressStateTimeline> = LazyLock::new(|| {
    timeline!(
        ProgressState 2.50s
        // first expanding cycle
        from { x1: 0.0, x2: 0.0 }
        10% { x1: 0.0, x2: 0.3 }
        30% { x1: 0.5, x2: 1.0 }
        50% { x1: 1.0, x2: 1.0 }
        // second contracting cycle
        60% { x1: 0.0, x2: 0.0 }
        70% { x1: 0.0, x2: 0.5 }
        80% { x1: 0.5, x2: 0.8 }
        90% { x1: 0.85, x2: 1.0 }
        to { x1: 1.0, x2: 1.0 }
    )
});

pub struct SkiaProgressBar {
    pub state: ProgressState,
    pub is_indeterminate: bool,
    current_time: f32,
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
    }

    /// Advance the timeline by `elapsed_secs`; returns whether the visible state changed.
    fn advance(&mut self, elapsed_secs: f32) -> bool {
        if self.is_indeterminate {
            let before = self.state.clone();
            INDETERMINATE_TIMELINE.update(&mut self.state, self.current_time);
            self.current_time += elapsed_secs;
            if self.current_time > 2.6 {
                self.current_time = 0.0;
            }
            self.state != before
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

    fn paint(&mut self, pm: &mut PixmapMut, ctx: &PaintCtx) {
        let s = ctx.scale;
        let (x, y, w, h) = (
            self.bounds.x * s,
            self.bounds.y * s,
            self.bounds.w * s,
            self.bounds.h * s,
        );
        let radius = 2.0 * s;

        // Clear our own bounds to the background.
        fill_rect(pm, x, y, w, h, ctx.theme.color_background);

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

        self.dirty = false;
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
