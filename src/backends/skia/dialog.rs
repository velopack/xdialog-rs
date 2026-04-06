use std::num::NonZeroU32;
use std::sync::Arc;

use softbuffer::Surface;
use tiny_skia::Pixmap;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

use crate::model::*;

use super::button::SkiaButton;
use super::font::{FONT_BOLD, FONT_REGULAR};
use super::icons;
use super::progress::SkiaProgressBar;
use super::renderer::{fill_rect, fill_rounded_rect, stroke_rounded_rect};
use super::text::{layout_text, measure_text_width, render_text};
use super::theme::SkiaTheme;

const BODY_SIZE: f32 = 14.0;
const TITLE_SIZE: f32 = 18.0;
const MIN_WIDTH: i32 = 350;
const MAX_WIDTH: i32 = 600;

pub struct SkiaDialog {
    pub window: Arc<Window>,
    surface: Surface<Arc<Window>, Arc<Window>>,
    theme: SkiaTheme,
    options: XDialogOptions,
    buttons: Vec<SkiaButton>,
    progress: Option<SkiaProgressBar>,
    result_sender: Option<oneshot::Sender<XDialogResult>>,
    needs_redraw: bool,
    scale_factor: f64,
    has_progress: bool,
    // Layout metrics (logical pixels)
    content_width: i32,
    content_height: i32,
    has_icon: bool,
}

impl SkiaDialog {
    pub fn new(
        event_loop: &ActiveEventLoop,
        options: XDialogOptions,
        theme: &SkiaTheme,
        has_progress: bool,
        result_sender: oneshot::Sender<XDialogResult>,
    ) -> Self {
        let has_icon = options.icon != XDialogIcon::None;

        // Compute window size
        let (win_w, win_h) = compute_window_size(&options, theme, has_progress, has_icon);

        let attrs = WindowAttributes::default()
            .with_title(options.title.clone())
            .with_inner_size(LogicalSize::new(win_w as f64, win_h as f64))
            .with_resizable(false);

        let window = Arc::new(event_loop.create_window(attrs).unwrap());
        let scale_factor = window.scale_factor();

        let context = softbuffer::Context::new(window.clone()).unwrap();
        let surface = Surface::new(&context, window.clone()).unwrap();

        // Create buttons
        let mut buttons = Vec::new();
        let button_iter: Vec<(usize, &String)> = if theme.button_order_reversed {
            options.buttons.iter().enumerate().rev().collect()
        } else {
            options.buttons.iter().enumerate().collect()
        };
        for (index, label) in button_iter {
            buttons.push(SkiaButton::new(label, index, theme));
        }

        let progress = if has_progress {
            Some(SkiaProgressBar::new())
        } else {
            None
        };

        let mut dialog = Self {
            window,
            surface,
            theme: theme.clone(),
            options,
            buttons,
            progress,
            result_sender: Some(result_sender),
            needs_redraw: true,
            scale_factor,
            has_progress,
            content_width: win_w,
            content_height: win_h,
            has_icon,
        };

        dialog.layout_buttons();
        dialog
    }

    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw
    }

    pub fn send_result(&mut self, result: XDialogResult) {
        if let Some(sender) = self.result_sender.take() {
            let _ = sender.send(result);
        }
    }

    pub fn close(&mut self) {
        self.send_result(XDialogResult::WindowClosed);
        self.window.set_visible(false);
    }

    pub fn set_body_text(&mut self, text: &str) {
        self.options.message = text.to_string();
        let (win_w, win_h) =
            compute_window_size(&self.options, &self.theme, self.has_progress, self.has_icon);
        self.content_width = win_w;
        self.content_height = win_h;
        let _ = self
            .window
            .request_inner_size(LogicalSize::new(win_w as f64, win_h as f64));
        self.layout_buttons();
        self.needs_redraw = true;
    }

    pub fn set_progress_value(&mut self, value: f32) {
        if let Some(p) = &mut self.progress {
            p.set_value(value);
            self.needs_redraw = true;
        }
    }

    pub fn set_progress_indeterminate(&mut self) {
        if let Some(p) = &mut self.progress {
            p.set_indeterminate();
            self.needs_redraw = true;
        }
    }

    pub fn handle_resized(&mut self, size: PhysicalSize<u32>) {
        let _ = size;
        self.needs_redraw = true;
    }

    pub fn handle_scale_factor_changed(&mut self, scale_factor: f64) {
        self.scale_factor = scale_factor;
        self.layout_buttons();
        self.needs_redraw = true;
    }

    pub fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        let lx = (position.x / self.scale_factor) as f32;
        let ly = (position.y / self.scale_factor) as f32;

        for btn in &mut self.buttons {
            let was_hovered = btn.hovered;
            btn.set_hovered(btn.hit_test(lx, ly));
            if btn.hovered != was_hovered {
                self.needs_redraw = true;
            }
        }
    }

    pub fn handle_mouse_pressed(&mut self) {
        for btn in &mut self.buttons {
            if btn.hovered {
                btn.set_pressed(true);
                self.needs_redraw = true;
            }
        }
    }

    pub fn handle_mouse_released(&mut self) -> Option<usize> {
        let mut clicked_index = None;
        for btn in &mut self.buttons {
            if btn.pressed && btn.hovered {
                clicked_index = Some(btn.index);
            }
            btn.set_pressed(false);
        }
        self.needs_redraw = true;
        clicked_index
    }

    pub fn handle_close_requested(&mut self) {
        self.send_result(XDialogResult::WindowClosed);
    }

    /// Advance animations. Returns true if a redraw is needed.
    pub fn tick(&mut self, elapsed: f32) -> bool {
        for btn in &mut self.buttons {
            if btn.tick(elapsed) {
                self.needs_redraw = true;
            }
        }
        if let Some(p) = &mut self.progress {
            if p.tick(elapsed) {
                self.needs_redraw = true;
            }
        }
        self.needs_redraw
    }

    fn layout_buttons(&mut self) {
        if self.buttons.is_empty() {
            return;
        }

        let theme = &self.theme;
        let panel_y = (self.content_height - theme.button_panel_height) as f32;
        let panel_h = theme.button_panel_height as f32;
        let margin = theme.button_panel_margin as f32;
        let spacing = theme.button_panel_spacing as f32;

        // Measure button widths
        let mut total_btn_width: f32 = 0.0;
        let mut btn_widths = Vec::new();
        for btn in &self.buttons {
            let text_w = measure_text_width(&btn.label, &FONT_REGULAR, BODY_SIZE);
            let btn_w = text_w + (theme.button_text_padding * 2) as f32;
            btn_widths.push(btn_w);
            total_btn_width += btn_w;
        }
        total_btn_width += spacing * (self.buttons.len() as f32 - 1.0);

        // Right-align buttons within panel
        let mut x = self.content_width as f32 - margin - total_btn_width;

        for (i, btn) in self.buttons.iter_mut().enumerate() {
            let btn_h = panel_h - margin * 2.0;
            btn.set_bounds(x, panel_y + margin, btn_widths[i], btn_h);
            x += btn_widths[i] + spacing;
        }
    }

    pub fn render_and_present(&mut self) {
        if !self.needs_redraw {
            return;
        }
        self.needs_redraw = false;

        let phys_size = self.window.inner_size();
        let pw = phys_size.width;
        let ph = phys_size.height;

        if pw == 0 || ph == 0 {
            return;
        }

        let scale = self.scale_factor as f32;

        let mut pixmap = match Pixmap::new(pw, ph) {
            Some(p) => p,
            None => return,
        };

        // 1. Fill background
        fill_rect(
            &mut pixmap.as_mut(),
            0.0,
            0.0,
            pw as f32,
            ph as f32,
            self.theme.color_background,
        );

        let theme = &self.theme;

        // Layout: two columns (icon | text), uniform gap between all vertical elements.
        // gap = default_content_margin used as the single consistent spacing value.
        //
        //  [gap]                          <- top margin
        //  [icon]  [main instruction]     <- icon top == text top
        //          [gap]
        //          [progress bar]
        //          [gap]
        //          [body text]
        //  [gap]                          <- bottom margin (before button panel)
        //  [button panel]

        let gap = theme.default_content_margin as f32 * scale;
        let icon_size = theme.main_icon_size as f32 * scale;
        let prog_h = 6.0 * scale;

        // Text column X position
        let mut text_x = gap;
        if self.has_icon {
            text_x = gap + icon_size + gap;
        }
        let text_w = pw as f32 - text_x - gap;

        // Vertical cursor starts after top margin
        let mut y = gap;

        // 2. Draw icon directly onto dialog pixmap (no intermediate pixmap = clean AA)
        if self.has_icon {
            icons::draw_icon(&mut pixmap.as_mut(), &self.options.icon, gap, y, icon_size);
        }

        // 3. Draw title text
        if !self.options.main_instruction.is_empty() {
            let title_layout =
                layout_text(&self.options.main_instruction, &FONT_BOLD, TITLE_SIZE * scale, text_w);
            render_text(
                &mut pixmap.as_mut(),
                &title_layout,
                &FONT_BOLD,
                TITLE_SIZE * scale,
                theme.color_title_text,
                text_x,
                y,
            );
            y += title_layout.total_height;
            y += gap;
        }

        // 4. Draw progress bar
        if let Some(ref progress) = self.progress {
            let prog_x = text_x;
            let prog_w = text_w;

            fill_rounded_rect(
                &mut pixmap.as_mut(),
                prog_x, y, prog_w, prog_h,
                2.0 * scale,
                theme.color_progress_background,
            );

            let bar_start = progress.state.x1 * prog_w;
            let bar_end = progress.state.x2 * prog_w;
            let bar_w = bar_end - bar_start;
            if bar_w > 0.0 {
                fill_rounded_rect(
                    &mut pixmap.as_mut(),
                    prog_x + bar_start, y, bar_w, prog_h,
                    2.0 * scale,
                    theme.color_progress_foreground,
                );
            }

            y += prog_h;
            y += gap;
        }

        // 5. Draw body text
        if !self.options.message.is_empty() {
            let body_layout =
                layout_text(&self.options.message, &FONT_REGULAR, BODY_SIZE * scale, text_w);
            render_text(
                &mut pixmap.as_mut(),
                &body_layout,
                &FONT_REGULAR,
                BODY_SIZE * scale,
                theme.color_body_text,
                text_x,
                y,
            );
        }

        // 6. Draw button panel background
        if !self.buttons.is_empty() {
            let panel_y = (self.content_height - theme.button_panel_height) as f32 * scale;
            fill_rect(
                &mut pixmap.as_mut(),
                0.0,
                panel_y,
                pw as f32,
                theme.button_panel_height as f32 * scale,
                theme.color_background_alt,
            );
        }

        // 7. Draw buttons
        for btn in &self.buttons {
            let colors = btn.current_colors();
            let bx = btn.x * scale;
            let by = btn.y * scale;
            let bw = btn.width * scale;
            let bh = btn.height * scale;
            let radius = colors.border_radius as f32 * scale;

            // Fill
            fill_rounded_rect(
                &mut pixmap.as_mut(),
                bx,
                by,
                bw,
                bh,
                radius,
                (colors.fill_r, colors.fill_g, colors.fill_b),
            );

            // Border
            if colors.border_width > 0 {
                stroke_rounded_rect(
                    &mut pixmap.as_mut(),
                    bx,
                    by,
                    bw,
                    bh,
                    radius,
                    (colors.border_r, colors.border_g, colors.border_b),
                    colors.border_width as f32 * scale,
                );
            }

            // Label text (centered)
            let label_layout =
                layout_text(&btn.label, &FONT_REGULAR, BODY_SIZE * scale, bw);
            let text_x = bx + (bw - label_layout.total_width) / 2.0;
            let text_y = by + (bh - label_layout.total_height) / 2.0;
            render_text(
                &mut pixmap.as_mut(),
                &label_layout,
                &FONT_REGULAR,
                BODY_SIZE * scale,
                (colors.text_r, colors.text_g, colors.text_b),
                text_x,
                text_y,
            );
        }

        // 8. Present to surface
        self.surface
            .resize(
                NonZeroU32::new(pw).unwrap(),
                NonZeroU32::new(ph).unwrap(),
            )
            .unwrap();

        let mut buffer = self.surface.buffer_mut().unwrap();
        let src = pixmap.data();

        // Convert RGBA (tiny-skia) to native format (0xAARRGGBB on most platforms)
        for i in 0..(pw * ph) as usize {
            let si = i * 4;
            let r = src[si] as u32;
            let g = src[si + 1] as u32;
            let b = src[si + 2] as u32;
            let a = src[si + 3] as u32;
            buffer[i] = (a << 24) | (r << 16) | (g << 8) | b;
        }

        buffer.present().unwrap();
    }
}

fn compute_window_size(
    options: &XDialogOptions,
    theme: &SkiaTheme,
    has_progress: bool,
    has_icon: bool,
) -> (i32, i32) {
    let gap = theme.default_content_margin;
    let prog_h = 6;

    // Horizontal: gap + [icon + gap] + text + gap
    let pad_x = if has_icon {
        gap + theme.main_icon_size + gap + gap
    } else {
        gap + gap
    };

    // Measure unwrapped text widths for width calculation
    let title_width = measure_text_width(&options.main_instruction, &FONT_BOLD, TITLE_SIZE) as i32;
    let body_width = measure_text_width(&options.message, &FONT_REGULAR, BODY_SIZE) as i32;
    let initial_width = body_width.max(title_width);

    let window_width = if initial_width <= 600 {
        300
    } else if initial_width >= 4000 {
        600
    } else {
        300 + (((initial_width - 600) as f32 / 3400.0) * 300.0) as i32
    };

    let final_width = window_width.clamp(MIN_WIDTH, MAX_WIDTH);

    // Compute wrapped text heights at final width
    let text_w = (final_width - pad_x) as f32;
    let wrapped_title = layout_text(&options.main_instruction, &FONT_BOLD, TITLE_SIZE, text_w);
    let wrapped_body = layout_text(&options.message, &FONT_REGULAR, BODY_SIZE, text_w);

    // Vertical: gap + [title + gap] + [progress + gap] + [body + gap] + [button_panel]
    let mut h = gap; // top margin
    if !options.main_instruction.is_empty() {
        h += wrapped_title.total_height as i32;
        h += gap;
    }
    if has_progress {
        h += prog_h;
        h += gap;
    }
    if !options.message.is_empty() {
        h += wrapped_body.total_height as i32;
        h += gap;
    }
    // Icon needs gap + icon_size + gap vertically
    if has_icon {
        h = h.max(gap + theme.main_icon_size + gap);
    }
    if !options.buttons.is_empty() {
        h += theme.button_panel_height;
    }

    (final_width, h)
}
