use std::cell::RefCell;
use std::rc::Rc;

use fltk::{app, draw::*, widget, widget_extends};
use fltk::enums::{Color, Event, FrameType, Key};
use fltk::prelude::{WidgetBase, WidgetExt};
use mina::{Animate, prelude::*};

use super::fltk_theme::DialogTheme;

#[derive(Animate, Clone, Debug, Default, PartialEq)]
struct ButtonColorState {
    border_radius: i32,
    border_width: i32,

    border_r: u8,
    border_g: u8,
    border_b: u8,

    fill_r: u8,
    fill_g: u8,
    fill_b: u8,

    text_r: u8,
    text_g: u8,
    text_b: u8,
}

pub struct CustomButton {
    inner: widget::Widget,
}

#[derive(Clone, Default, PartialEq, State)]
enum ButtonState {
    #[default] Idle,
    Hovered,
    Pressed,
    Focused,
}

fn calculate_offsets(rs: f64, num_points: usize) -> Vec<f64> {
    let mut offsets = Vec::with_capacity(num_points);
    for i in 0..num_points {
        let angle = std::f64::consts::PI * 0.5 * (i as f64 / (num_points - 1) as f64);
        offsets.push(rs - angle.sin() * rs); // Calculate outward arc
    }
    offsets
}

pub fn draw_improved_rbox(x: i32, y: i32, w: i32, h: i32, max_radius: i32, fill: bool, col: Color) {
    let max_radius = if max_radius < 0 { 0 } else { max_radius };
    let mut rs = w.min(h) * 2 / 5;  // Smallest side divided by 5
    if rs > max_radius {
        rs = max_radius;
    }
    rs = rs.max(1); // Ensure radius isn't zero

    let rs_f64 = rs as f64;
    let num_points = 5 + (rs_f64.sqrt() as usize); // Adjust number of points based on sqrt of radius
    let offsets = calculate_offsets(rs_f64, num_points);

    let x_f64 = x as f64;
    let y_f64 = y as f64;
    let w_f64 = w as f64;
    let h_f64 = h as f64;
    let old_col = get_color();

    set_draw_color(col);
    if fill {
        begin_polygon();
    } else {
        begin_loop();
    }
    // Upper left corner
    for i in 0..num_points {
        vertex(
            0.5 + x_f64 + offsets[num_points - i - 1],
            0.5 + y_f64 + offsets[i],
        );
    }

    // Upper right corner
    for i in 0..num_points {
        vertex(
            0.5 + x_f64 + w_f64 - 1.0 - offsets[i],
            0.5 + y_f64 + offsets[num_points - i - 1],
        );
    }

    // Lower right corner
    for i in 0..num_points {
        vertex(
            0.5 + x_f64 + w_f64 - 1.0 - offsets[num_points - i - 1],
            0.5 + y_f64 + h_f64 - 1.0 - offsets[i],
        );
    }

    // Lower left corner
    for i in 0..num_points {
        vertex(
            0.5 + x_f64 + offsets[i],
            0.5 + y_f64 + h_f64 - 1.0 - offsets[num_points - i - 1],
        );
    }

    if fill {
        end_polygon();
    } else {
        end_loop();
    }
    set_draw_color(old_col);
}

impl CustomButton {
    pub fn new(dialog_theme: &DialogTheme) -> Self {
        let mut inner = widget::Widget::default();
        inner.set_frame(FrameType::FlatBox);
        let mut inner2 = inner.clone();

        let animator = animator!(ButtonColorState {
            default(ButtonState::Idle, {
                border_radius: dialog_theme.style_button_inactive.border_radius,
                border_width: dialog_theme.style_button_inactive.border_width,
                border_r: dialog_theme.style_button_inactive.color_button_border.to_rgb().0,
                border_g: dialog_theme.style_button_inactive.color_button_border.to_rgb().1,
                border_b: dialog_theme.style_button_inactive.color_button_border.to_rgb().2,
                fill_r: dialog_theme.style_button_inactive.color_button_background.to_rgb().0,
                fill_g: dialog_theme.style_button_inactive.color_button_background.to_rgb().1,
                fill_b: dialog_theme.style_button_inactive.color_button_background.to_rgb().2,
                text_r: dialog_theme.style_button_inactive.color_button_text.to_rgb().0,
                text_g: dialog_theme.style_button_inactive.color_button_text.to_rgb().1,
                text_b: dialog_theme.style_button_inactive.color_button_text.to_rgb().2,
            }),
            ButtonState::Idle => 0.15s to default,
            ButtonState::Hovered => 0.15s to { 
                border_radius: dialog_theme.style_button_hover.border_radius,
                border_width: dialog_theme.style_button_hover.border_width,
                border_r: dialog_theme.style_button_hover.color_button_border.to_rgb().0,
                border_g: dialog_theme.style_button_hover.color_button_border.to_rgb().1,
                border_b: dialog_theme.style_button_hover.color_button_border.to_rgb().2,
                fill_r: dialog_theme.style_button_hover.color_button_background.to_rgb().0,
                fill_g: dialog_theme.style_button_hover.color_button_background.to_rgb().1,
                fill_b: dialog_theme.style_button_hover.color_button_background.to_rgb().2,
                text_r: dialog_theme.style_button_hover.color_button_text.to_rgb().0,
                text_g: dialog_theme.style_button_hover.color_button_text.to_rgb().1,
                text_b: dialog_theme.style_button_hover.color_button_text.to_rgb().2,
            },
            ButtonState::Pressed => 0.15s to {
                border_radius: dialog_theme.style_button_pressed.border_radius,
                border_width: dialog_theme.style_button_pressed.border_width,
                border_r: dialog_theme.style_button_pressed.color_button_border.to_rgb().0,
                border_g: dialog_theme.style_button_pressed.color_button_border.to_rgb().1,
                border_b: dialog_theme.style_button_pressed.color_button_border.to_rgb().2,
                fill_r: dialog_theme.style_button_pressed.color_button_background.to_rgb().0,
                fill_g: dialog_theme.style_button_pressed.color_button_background.to_rgb().1,
                fill_b: dialog_theme.style_button_pressed.color_button_background.to_rgb().2,
                text_r: dialog_theme.style_button_pressed.color_button_text.to_rgb().0,
                text_g: dialog_theme.style_button_pressed.color_button_text.to_rgb().1,
                text_b: dialog_theme.style_button_pressed.color_button_text.to_rgb().2,
            },
            ButtonState::Focused => 0.15s to {
                border_radius: dialog_theme.style_button_focused.border_radius,
                border_width: dialog_theme.style_button_focused.border_width,
                border_r: dialog_theme.style_button_focused.color_button_border.to_rgb().0,
                border_g: dialog_theme.style_button_focused.color_button_border.to_rgb().1,
                border_b: dialog_theme.style_button_focused.color_button_border.to_rgb().2,
                fill_r: dialog_theme.style_button_focused.color_button_background.to_rgb().0,
                fill_g: dialog_theme.style_button_focused.color_button_background.to_rgb().1,
                fill_b: dialog_theme.style_button_focused.color_button_background.to_rgb().2,
                text_r: dialog_theme.style_button_focused.color_button_text.to_rgb().0,
                text_g: dialog_theme.style_button_focused.color_button_text.to_rgb().1,
                text_b: dialog_theme.style_button_focused.color_button_text.to_rgb().2,
            },
        });

        let animator_cell1 = Rc::new(RefCell::new(animator));
        let animator_cell2 = animator_cell1.clone();
        let animator_cell3 = animator_cell1.clone();

        let hovered_cell1 = Rc::new(RefCell::new(false));
        let pressed_cell1 = Rc::new(RefCell::new(false));

        let update_interaction_state = move |set_hovered: Option<bool>, set_pressed: Option<bool>, is_focused: bool| -> ButtonState {
            let mut pressed = pressed_cell1.borrow_mut();
            let mut hovered = hovered_cell1.borrow_mut();

            if let Some(new_hovered) = set_hovered {
                *hovered = new_hovered;
            }

            if let Some(new_pressed) = set_pressed {
                *pressed = new_pressed;
            }

            if *pressed {
                ButtonState::Pressed
            } else if *hovered {
                ButtonState::Hovered
            } else if is_focused {
                ButtonState::Focused
            } else {
                ButtonState::Idle
            }
        };

        inner.handle(move |i, ev| match ev {
            Event::Push => {
                let mut animator = animator_cell1.borrow_mut();
                let new_state = update_interaction_state(None, Some(true), i.has_focus());
                animator.set_state(&new_state);
                true
            }
            Event::Released => {
                let mut animator = animator_cell1.borrow_mut();
                let new_state = update_interaction_state(None, Some(false), i.has_focus());
                animator.set_state(&new_state);
                i.do_callback();
                true
            }
            Event::KeyDown => {
                if app::event_key() == Key::Enter {
                    i.do_callback();
                    true
                } else {
                    false
                }
            }
            Event::Enter => {
                let mut animator = animator_cell1.borrow_mut();
                let new_state = update_interaction_state(Some(true), None, i.has_focus());
                animator.set_state(&new_state);
                true
            }
            Event::Leave => {
                let mut animator = animator_cell1.borrow_mut();
                let new_state = update_interaction_state(Some(false), None, i.has_focus());
                animator.set_state(&new_state);
                true
            }
            Event::Focus => {
                let mut animator = animator_cell1.borrow_mut();
                let new_state = update_interaction_state(None, None, true);
                animator.set_state(&new_state);
                true
            }
            Event::Unfocus => {
                let mut animator = animator_cell1.borrow_mut();
                let new_state = update_interaction_state(None, None, false);
                animator.set_state(&new_state);
                true
            }
            _ => false,
        });

        inner.draw(move |i| {
            let animator = animator_cell2.borrow();
            let state = animator.current_values();

            draw_box(FrameType::FlatBox, i.x(), i.y(), i.w(), i.h(), Color::Background2);
            draw_improved_rbox(i.x(), i.y(), i.w(), i.h(), state.border_radius, true, Color::from_rgb(state.fill_r, state.fill_g, state.fill_b));
            
            if state.border_width > 0 {
                set_line_style(LineStyle::Solid, state.border_width);
                draw_improved_rbox(i.x(), i.y(), i.w(), i.h(), state.border_radius, false, Color::from_rgb(state.border_r, state.border_g, state.border_b));
                set_line_style(LineStyle::Solid, 1);
            }

            set_font(i.label_font(), i.label_size());
            set_draw_color(Color::from_rgb(state.text_r, state.text_g, state.text_b));
            draw_text2(&i.label(), i.x(), i.y(), i.w(), i.h(), i.align());
        });

        app::add_timeout3(0.016, move |handle| {
            let mut animator = animator_cell3.borrow_mut();
            animator.advance(0.016);
            inner2.redraw();
            app::repeat_timeout3(0.016, handle);
        });

        Self {
            inner,
        }
    }
}

widget_extends!(CustomButton, widget::Widget, inner);