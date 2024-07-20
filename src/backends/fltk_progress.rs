use std::cell::RefCell;
use std::rc::Rc;
use std::time::SystemTime;

use fltk::{*, prelude::*};
use fltk::enums::{Color, FrameType};
use mina::prelude::*;
use crate::backends::fltk_theme::DialogTheme;

#[derive(Animate, Clone, Debug, Default, PartialEq)]
struct ProgressIndeterminateState {
    x1: f32,
    x2: f32,
}

pub struct CustomProgressBar {
    inner: widget::Widget,
    // current_value: RwLock<f32>,
    indeterminate: Rc<RefCell<ProgressIndeterminateState>>,
}

impl CustomProgressBar {
    // our constructor
    pub fn new(dialog_theme: &DialogTheme) -> Self {
        // let indeterminate_timeline = timeline!(
        //     ProgressIndeterminateState 1.15s 
        //     from { x1: 0.0, x2: 0.0 }
        //     20% { x1: 0.0, x2: 0.3 }
        //     60% { x1: 0.5, x2: 1.0 }
        //     to { x1: 1.0, x2: 1.0 }
        // );

        let indeterminate_timeline = timeline!(
            ProgressIndeterminateState 2.50s
            from { x1: 0.0, x2: 0.0 }
            10% { x1: 0.0, x2: 0.3 }
            30% { x1: 0.5, x2: 1.0 }
            50% { x1: 1.0, x2: 1.0 }
            60% { x1: 0.0, x2: 0.0 }
            70% { x1: 0.0, x2: 0.5 }
            80% { x1: 0.5, x2: 0.8 }
            90% { x1: 0.85, x2: 1.0 }
            to { x1: 1.0, x2: 1.0 }
        );

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

        // let root_state = Rc::new(RefCell::new(ProgressIndeterminateState::default()));

        // get animation start time (current time)
        let start_time = Rc::new(RefCell::new(SystemTime::now()));

        let mut inner_clone = inner.clone();
        let rs2 = root_state.clone();
        let st1 = start_time.clone();
        app::add_timeout3(0.008, move |handle| {
            let mut indeterminate_state = rs2.borrow_mut();
            let indeterminate_state = &mut *indeterminate_state;

            let mut start = st1.borrow_mut();
            let elapsed = start.elapsed().unwrap().as_secs_f32();
            if elapsed > 2.6 {
                *start = SystemTime::now();
            }

            indeterminate_timeline.update(indeterminate_state, elapsed);
            inner_clone.redraw();

            app::repeat_timeout3(0.008, handle);
        });

        Self {
            inner,
            // current_value,
            indeterminate: root_state,
        }
    }

    // get the times our button was clicked
    // pub fn set_value(&mut self, value: f32) {
    //     *self.current_value.borrow_mut() = value;
    // }
}

widget_extends!(CustomProgressBar, widget::Widget, inner);

// fltk::macros::widget::impl_widget_base!(CustomProgressBar, Fl_Box);
// fltk::macros::widget::impl_widget_default!(CustomProgressBar);
// fltk::macros::widget::impl_widget_ext!(CustomProgressBar, Fl_Box);

