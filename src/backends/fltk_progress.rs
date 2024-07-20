use std::cell::RefCell;
use std::rc::Rc;

use fltk::{*, prelude::*};
use fltk::enums::{Color, FrameType};
use mina::prelude::*;

use super::{fltk::Tick, fltk_theme::DialogTheme};

#[derive(Animate, Clone, Debug, Default, PartialEq)]
struct ProgressIndeterminateState {
    x1: f32,
    x2: f32,
}

pub struct CustomProgressBar {
    inner: widget::Widget,
    state: Rc<RefCell<ProgressIndeterminateState>>,
    is_indeterminate: bool,
    current_time: f32,
    value_animator: Option<ProgressIndeterminateStateTimeline>,
}

lazy_static! {
    static ref INDETERMINATE_TIMELINE: ProgressIndeterminateStateTimeline = timeline!(
        ProgressIndeterminateState 2.50s
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
    );
}

impl CustomProgressBar {
    // our constructor
    pub fn new(dialog_theme: &DialogTheme) -> Self {
        let mut inner = widget::Widget::default();
        inner.set_frame(FrameType::FlatBox);

        // let current_value = RwLock::new(0.0);
        let root_state = Rc::new(RefCell::new(ProgressIndeterminateState::default()));

        let rs1 = root_state.clone();
        let theme = dialog_theme.clone();
        inner.draw(move |i| { // we need a draw implementation
            draw::draw_box(FrameType::FlatBox, i.x(), i.y(), i.w(), i.h(), Color::BackGround);

            // just a hack to work around anti-aliasing
            draw::draw_rbox(i.x(), i.y(), i.w(), i.h(), 2, true, theme.color_progress_background);
            draw::draw_rbox(i.x(), i.y(), i.w(), i.h(), 2, true, theme.color_progress_background);
            draw::draw_rbox(i.x(), i.y(), i.w(), i.h(), 2, true, theme.color_progress_background);

            let state = rs1.borrow();

            let start = (state.x1 * i.w() as f32) as i32 + i.x();
            let end = (state.x2 * i.w() as f32) as i32 + i.x();
            let width = end - start;

            if width > 0 {
                // just a hack to work around anti-aliasing
                draw::draw_rbox(start, i.y(), width, i.h(), 2, true, theme.color_progress_foreground);
                draw::draw_rbox(start, i.y(), width, i.h(), 2, true, theme.color_progress_foreground);
                draw::draw_rbox(start, i.y(), width, i.h(), 2, true, theme.color_progress_foreground);
            }
        });

        Self {
            inner,
            state: root_state,
            current_time: 0.0,
            is_indeterminate: false,
            value_animator: None,
        }
    }

    pub fn set_value(&mut self, value: f32) {
        let state = self.state.borrow();

        let animation: ProgressIndeterminateStateTimeline = timeline!(
            ProgressIndeterminateState 0.3s Easing::OutCubic
            from { x1: 0.0, x2: state.x2 }
            to { x1: 0.0, x2: value }
        );

        self.is_indeterminate = false;
        self.current_time = 0.0;
        self.value_animator = Some(animation);
        self.inner.redraw();
    }

    pub fn set_indeterminate(&mut self) {
        self.is_indeterminate = true;
        self.current_time = 0.0;
        self.inner.redraw();
    }
}

widget_extends!(CustomProgressBar, widget::Widget, inner);

impl Tick for CustomProgressBar {
    fn tick(&mut self, elapsed_secs: f32) {
        let mut state = self.state.borrow_mut();
        if self.is_indeterminate {
            INDETERMINATE_TIMELINE.update(&mut *state, self.current_time);
            self.current_time += elapsed_secs;

            if self.current_time > 2.6 {
                self.current_time = 0.0;
            }
            self.inner.redraw();
        } else if let Some(ref mut animator) = self.value_animator {
            animator.update(&mut *state, self.current_time);
            self.current_time += elapsed_secs;

            if self.current_time > 0.3 {
                self.value_animator = None;
            }
            self.inner.redraw();
        }
    }
}
