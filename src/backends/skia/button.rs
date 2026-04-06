use std::cell::RefCell;
use std::rc::Rc;

use mina::prelude::*;

use super::theme::SkiaTheme;

#[derive(Animate, Clone, Debug, Default, PartialEq)]
pub struct ButtonColorState {
    pub border_radius: i32,
    pub border_width: i32,

    pub border_r: u8,
    pub border_g: u8,
    pub border_b: u8,

    pub fill_r: u8,
    pub fill_g: u8,
    pub fill_b: u8,

    pub text_r: u8,
    pub text_g: u8,
    pub text_b: u8,
}

#[derive(Clone, Default, PartialEq, State)]
pub enum ButtonState {
    #[default]
    Idle,
    Hovered,
    Pressed,
    Focused,
}

pub struct SkiaButton {
    pub label: String,
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
    animator: Rc<RefCell<Box<dyn StateAnimator<State = ButtonState, Values = ButtonColorState>>>>,
}

impl SkiaButton {
    pub fn new(label: &str, index: usize, theme: &SkiaTheme) -> Self {
        let animator = animator!(ButtonColorState {
            default(ButtonState::Idle, {
                border_radius: theme.style_button_inactive.border_radius,
                border_width: theme.style_button_inactive.border_width,
                border_r: theme.style_button_inactive.border_color.0,
                border_g: theme.style_button_inactive.border_color.1,
                border_b: theme.style_button_inactive.border_color.2,
                fill_r: theme.style_button_inactive.background_color.0,
                fill_g: theme.style_button_inactive.background_color.1,
                fill_b: theme.style_button_inactive.background_color.2,
                text_r: theme.style_button_inactive.text_color.0,
                text_g: theme.style_button_inactive.text_color.1,
                text_b: theme.style_button_inactive.text_color.2,
            }),
            ButtonState::Idle => 0.15s to default,
            ButtonState::Hovered => 0.15s to {
                border_radius: theme.style_button_hover.border_radius,
                border_width: theme.style_button_hover.border_width,
                border_r: theme.style_button_hover.border_color.0,
                border_g: theme.style_button_hover.border_color.1,
                border_b: theme.style_button_hover.border_color.2,
                fill_r: theme.style_button_hover.background_color.0,
                fill_g: theme.style_button_hover.background_color.1,
                fill_b: theme.style_button_hover.background_color.2,
                text_r: theme.style_button_hover.text_color.0,
                text_g: theme.style_button_hover.text_color.1,
                text_b: theme.style_button_hover.text_color.2,
            },
            ButtonState::Pressed => 0.15s to {
                border_radius: theme.style_button_pressed.border_radius,
                border_width: theme.style_button_pressed.border_width,
                border_r: theme.style_button_pressed.border_color.0,
                border_g: theme.style_button_pressed.border_color.1,
                border_b: theme.style_button_pressed.border_color.2,
                fill_r: theme.style_button_pressed.background_color.0,
                fill_g: theme.style_button_pressed.background_color.1,
                fill_b: theme.style_button_pressed.background_color.2,
                text_r: theme.style_button_pressed.text_color.0,
                text_g: theme.style_button_pressed.text_color.1,
                text_b: theme.style_button_pressed.text_color.2,
            },
            ButtonState::Focused => 0.15s to {
                border_radius: theme.style_button_focused.border_radius,
                border_width: theme.style_button_focused.border_width,
                border_r: theme.style_button_focused.border_color.0,
                border_g: theme.style_button_focused.border_color.1,
                border_b: theme.style_button_focused.border_color.2,
                fill_r: theme.style_button_focused.background_color.0,
                fill_g: theme.style_button_focused.background_color.1,
                fill_b: theme.style_button_focused.background_color.2,
                text_r: theme.style_button_focused.text_color.0,
                text_g: theme.style_button_focused.text_color.1,
                text_b: theme.style_button_focused.text_color.2,
            },
        });

        Self {
            label: label.to_string(),
            index,
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            hovered: false,
            pressed: false,
            focused: false,
            animator: Rc::new(RefCell::new(Box::new(animator))),
        }
    }

    pub fn set_bounds(&mut self, x: f32, y: f32, width: f32, height: f32) {
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height;
    }

    pub fn hit_test(&self, mx: f32, my: f32) -> bool {
        mx >= self.x && mx <= self.x + self.width && my >= self.y && my <= self.y + self.height
    }

    pub fn set_hovered(&mut self, hovered: bool) {
        self.hovered = hovered;
        self.update_state();
    }

    pub fn set_pressed(&mut self, pressed: bool) {
        self.pressed = pressed;
        self.update_state();
    }

    fn update_state(&mut self) {
        let state = if self.pressed {
            ButtonState::Pressed
        } else if self.hovered {
            ButtonState::Hovered
        } else if self.focused {
            ButtonState::Focused
        } else {
            ButtonState::Idle
        };
        self.animator.borrow_mut().set_state(&state);
    }

    pub fn current_colors(&self) -> ButtonColorState {
        self.animator.borrow().current_values().clone()
    }

    /// Advance animation. Returns true if the visual state changed.
    pub fn tick(&mut self, elapsed: f32) -> bool {
        let before = self.animator.borrow().current_values().clone();
        self.animator.borrow_mut().advance(elapsed);
        let after = self.animator.borrow().current_values().clone();
        before != after
    }
}
