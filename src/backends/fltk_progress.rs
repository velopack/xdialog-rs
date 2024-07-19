use fltk::{prelude::*, *};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::SystemTime;
use fltk::enums::Color;
use fltk::frame::Frame;
use mina::prelude::*;


use fltk::utils::FlString;
use fltk_sys::frame::*;
use std::ffi::{CStr, CString};
use std::sync::RwLock;

#[derive(Animate, Clone, Debug, Default, PartialEq)]
struct ProgressIndeterminateState {
    x1: f32,
    x2: f32,
}

// #[derive(Debug)]
// pub struct Frame {
//     inner: fltk::widget::WidgetTracker,
//     is_derived: bool,
// }


pub struct CustomProgressBar {
    inner: widget::Widget,
    // current_value: RwLock<f32>,
    indeterminate: Rc<RefCell<ProgressIndeterminateState>>,
}

impl CustomProgressBar {
    // our constructor
    pub fn new() -> Self {
        let indeterminate_timeline = timeline!(
            ProgressIndeterminateState 1.15s Repeat::Infinite Easing::InCubic
            from { x1: 0.0, x2: 0.0 }
            60% { x1: 0.5, x2: 1.0 }
            to { x1: 1.0, x2: 1.0 }
        );

        let mut inner = widget::Widget::default();
        inner.set_frame(enums::FrameType::FlatBox);

        // let current_value = RwLock::new(0.0);
        let mut root_state = Rc::new(RefCell::new(ProgressIndeterminateState::default()));

        let rs1 = root_state.clone();
        inner.draw(move |i| { // we need a draw implementation
            draw::draw_box(i.frame(), i.x(), i.y(), i.w(), i.h(), Color::Red);

            let state = rs1.borrow();

            let start = (state.x1 * i.w() as f32) as i32 + i.x();
            let end = (state.x2 * i.w() as f32) as i32 + i.x();
            let width = end - start;
            draw::draw_box(enums::FrameType::FlatBox, start, i.y(), width, i.h(), Color::Blue);


            // draw::set_draw_color(enums::Color::Black); // for the text
            // draw::set_font(enums::Font::Helvetica, app::font_size());
            // draw::draw_text2(&i.label(), i.x(), i.y(), i.w(), i.h(), i.align());
        });

        // let root_state = Rc::new(RefCell::new(ProgressIndeterminateState::default()));

        // get animation start time (current time)
        let start_time = Rc::new(RefCell::new(SystemTime::now()));

        let mut inner_clone = inner.clone();
        let rs2 = root_state.clone();
        let st1 = start_time.clone();
        app::add_timeout3(0.016, move |handle| {
            let mut indeterminate_state = rs2.borrow_mut();
            let indeterminate_state = &mut *indeterminate_state;
            
            let mut start = st1.borrow_mut();
            
            
            let elapsed = start.elapsed().unwrap().as_secs_f32();
            if elapsed > 1.30 {
                *start = SystemTime::now();
            }
            
            indeterminate_timeline.update(indeterminate_state, elapsed);
            inner_clone.redraw();

            app::repeat_timeout3(0.016, handle);
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

impl Default for CustomProgressBar {
    fn default() -> Self {
        CustomProgressBar::new()
    }
}

widget_extends!(CustomProgressBar, widget::Widget, inner);

// fltk::macros::widget::impl_widget_base!(CustomProgressBar, Fl_Box);
// fltk::macros::widget::impl_widget_default!(CustomProgressBar);
// fltk::macros::widget::impl_widget_ext!(CustomProgressBar, Fl_Box);

