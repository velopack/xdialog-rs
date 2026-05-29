use std::cell::RefCell;
use std::rc::Rc;

use mina::prelude::*;
use tiny_skia::PixmapMut;

use super::component::{Component, LayoutCtx, PaintCtx, Rect, Role, Size, BODY_SIZE};
use super::font::FONT_REGULAR;
use super::renderer::{fill_rect, fill_rounded_rect, stroke_rounded_rect};
use super::text::{layout_text, measure_text_width, render_text};
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
    bounds: Rect,
    hovered: bool,
    pressed: bool,
    focused: bool,
    dirty: bool,
    animating: bool,
    current_state: ButtonState,
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
            bounds: Rect::default(),
            hovered: false,
            pressed: false,
            focused: false,
            dirty: true,
            animating: false,
            current_state: ButtonState::Idle,
            animator: Rc::new(RefCell::new(Box::new(animator))),
        }
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
        if state != self.current_state {
            self.current_state = state.clone();
            self.animating = true;
        }
        self.animator.borrow_mut().set_state(&state);
    }

    fn current_colors(&self) -> ButtonColorState {
        self.animator.borrow().current_values().clone()
    }
}

impl Component for SkiaButton {
    fn role(&self) -> Role {
        Role::Button
    }

    fn measure(&mut self, ctx: &LayoutCtx) -> Size {
        let text_w = measure_text_width(&self.label, &FONT_REGULAR, BODY_SIZE);
        Size {
            w: text_w + (ctx.theme.button_text_padding * 2) as f32,
            h: (ctx.theme.button_panel_height - ctx.theme.button_panel_margin * 2) as f32,
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
        let colors = self.current_colors();
        let (bx, by, bw, bh) = (
            self.bounds.x * s,
            self.bounds.y * s,
            self.bounds.w * s,
            self.bounds.h * s,
        );
        let radius = colors.border_radius as f32 * s;

        // Clear to the footer colour the button sits on, then draw fill + border + label.
        fill_rect(pm, bx, by, bw, bh, ctx.theme.color_background_alt);

        fill_rounded_rect(
            pm,
            bx,
            by,
            bw,
            bh,
            radius,
            (colors.fill_r, colors.fill_g, colors.fill_b),
        );

        if colors.border_width > 0 {
            stroke_rounded_rect(
                pm,
                bx,
                by,
                bw,
                bh,
                radius,
                (colors.border_r, colors.border_g, colors.border_b),
                colors.border_width as f32 * s,
            );
        }

        let label_layout = layout_text(&self.label, &FONT_REGULAR, BODY_SIZE * s, bw);
        let text_x = bx + (bw - label_layout.total_width) / 2.0;
        let text_y = by + (bh - label_layout.total_height) / 2.0;
        render_text(
            pm,
            &label_layout,
            &FONT_REGULAR,
            BODY_SIZE * s,
            (colors.text_r, colors.text_g, colors.text_b),
            text_x,
            text_y,
        );

        self.dirty = false;
    }

    fn tick(&mut self, dt: f32) -> bool {
        let before = self.animator.borrow().current_values().clone();
        self.animator.borrow_mut().advance(dt);
        let after = self.animator.borrow().current_values().clone();
        let changed = before != after;
        if changed {
            self.dirty = true;
        } else {
            self.animating = false;
        }
        changed
    }

    fn is_animating(&self) -> bool {
        self.animating
    }

    fn focusable(&self) -> bool {
        true
    }

    fn set_hovered(&mut self, v: bool) {
        if self.hovered != v {
            self.hovered = v;
            self.update_state();
            self.dirty = true;
        }
    }

    fn set_pressed(&mut self, v: bool) {
        if self.pressed != v {
            self.pressed = v;
            self.update_state();
            self.dirty = true;
        }
    }

    fn set_focused(&mut self, v: bool) {
        if self.focused != v {
            self.focused = v;
            self.update_state();
            self.dirty = true;
        }
    }

    fn is_hovered(&self) -> bool {
        self.hovered
    }

    fn is_pressed(&self) -> bool {
        self.pressed
    }

    fn activation_index(&self) -> Option<usize> {
        Some(self.index)
    }
}
