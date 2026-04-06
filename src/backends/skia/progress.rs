use std::sync::LazyLock;

use mina::prelude::*;

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
}

impl SkiaProgressBar {
    pub fn new() -> Self {
        Self {
            state: ProgressState::default(),
            is_indeterminate: false,
            current_time: 0.0,
            value_animator: None,
        }
    }

    pub fn set_value(&mut self, value: f32) {
        let animation: ProgressStateTimeline = timeline!(
            ProgressState 0.3s Easing::OutCubic
            from { x1: 0.0, x2: self.state.x2 }
            to { x1: 0.0, x2: value }
        );

        self.is_indeterminate = false;
        self.current_time = 0.0;
        self.value_animator = Some(animation);
    }

    pub fn set_indeterminate(&mut self) {
        self.is_indeterminate = true;
        self.current_time = 0.0;
    }

    pub fn tick(&mut self, elapsed_secs: f32) -> bool {
        if self.is_indeterminate {
            INDETERMINATE_TIMELINE.update(&mut self.state, self.current_time);
            self.current_time += elapsed_secs;
            if self.current_time > 2.6 {
                self.current_time = 0.0;
            }
            true
        } else if let Some(ref mut animator) = self.value_animator {
            animator.update(&mut self.state, self.current_time);
            self.current_time += elapsed_secs;
            if self.current_time > 0.3 {
                self.value_animator = None;
            }
            true
        } else {
            false
        }
    }
}
