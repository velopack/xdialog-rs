use std::num::NonZeroU32;
use std::sync::Arc;

use softbuffer::Surface;
use tiny_skia::Pixmap;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

use winit::event::Modifiers;
use winit::keyboard::{Key, NamedKey};

use crate::model::*;

use super::button::SkiaButton;
use super::font::{FONT_BOLD, FONT_REGULAR};
use super::icons;
use super::progress::SkiaProgressBar;
use super::renderer::{fill_rect, fill_rounded_rect, stroke_rounded_rect};
use super::text::{layout_text, measure_text_width, render_text};
use super::theme::SkiaTheme;

pub enum KeyAction {
    None,
    ActivateButton(usize),
    Close,
}

const BODY_SIZE: f32 = 14.0;
const TITLE_SIZE: f32 = 18.0;
const MIN_WIDTH: i32 = 350;
const MAX_WIDTH: i32 = 600;

/// Physical-pixel rectangle cached after a full render for partial redraws.
#[derive(Clone, Copy, Default)]
struct PhysRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

pub struct SkiaDialog {
    pub window: Arc<Window>,
    surface: Surface<Arc<Window>, Arc<Window>>,
    pixmap: Option<Pixmap>,
    theme: SkiaTheme,
    options: XDialogOptions,
    buttons: Vec<SkiaButton>,
    focused_index: Option<usize>,
    shift_held: bool,
    progress: Option<SkiaProgressBar>,
    result_sender: Option<oneshot::Sender<XDialogResult>>,
    scale_factor: f64,
    has_progress: bool,
    content_width: i32,
    content_height: i32,
    has_icon: bool,
    // Dirty tracking
    dirty_full: bool,
    dirty_progress: bool,
    dirty_buttons: bool,
    // Cached rects (physical pixels) from last full render
    progress_rect: PhysRect,
    button_panel_rect: PhysRect,
    // Last size passed to surface.resize()
    last_surface_size: (u32, u32),
    // Cached ARGB buffer matching the softbuffer format, so partial redraws
    // only need to convert the dirty region and re-presents are a plain memcpy.
    present_cache: Vec<u32>,
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
        let (win_w, win_h) = compute_window_size(&options, theme, has_progress, has_icon);

        let mut attrs = WindowAttributes::default()
            .with_title(options.title.clone())
            .with_inner_size(LogicalSize::new(win_w as f64, win_h as f64))
            .with_resizable(false);

        // Pre-compute the centered position so the WM doesn't need to
        // reposition after mapping, which causes a visible jitter.
        if let Some(monitor) = event_loop
            .primary_monitor()
            .or_else(|| event_loop.available_monitors().next())
        {
            let mon_size = monitor.size();
            let mon_pos = monitor.position();
            let scale = monitor.scale_factor();
            let phys_w = (win_w as f64 * scale) as i32;
            let phys_h = (win_h as f64 * scale) as i32;
            let x = mon_pos.x + (mon_size.width as i32 - phys_w) / 2;
            let y = mon_pos.y + (mon_size.height as i32 - phys_h) / 2;
            attrs = attrs.with_position(PhysicalPosition::<i32>::new(x, y));
        }

        let window = Arc::new(event_loop.create_window(attrs).unwrap());
        let scale_factor = window.scale_factor();

        let context = softbuffer::Context::new(window.clone()).unwrap();
        let surface = Surface::new(&context, window.clone()).unwrap();

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
            pixmap: None,
            theme: theme.clone(),
            options,
            buttons,
            focused_index: None,
            shift_held: false,
            progress,
            result_sender: Some(result_sender),
            scale_factor,
            has_progress,
            content_width: win_w,
            content_height: win_h,
            has_icon,
            dirty_full: true,
            dirty_progress: false,
            dirty_buttons: false,
            progress_rect: PhysRect::default(),
            button_panel_rect: PhysRect::default(),
            last_surface_size: (0, 0),
            present_cache: Vec::new(),
        };

        dialog.layout_buttons();

        // Focus the last button by default
        if !dialog.buttons.is_empty() {
            let last = dialog.buttons.len() - 1;
            dialog.focused_index = Some(last);
            dialog.buttons[last].set_focused(true);
        }

        // Pre-render the first frame synchronously so the window has
        // content before the compositor/WM ever displays it.
        dialog.render_and_present();
        dialog
    }

    pub fn needs_redraw(&self) -> bool {
        self.dirty_full || self.dirty_progress || self.dirty_buttons
    }

    pub fn is_animating(&self) -> bool {
        if let Some(ref p) = self.progress {
            if p.is_animating() {
                return true;
            }
        }
        for btn in &self.buttons {
            if btn.is_animating() {
                return true;
            }
        }
        false
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
        self.dirty_full = true;
    }

    pub fn set_progress_value(&mut self, value: f32) {
        if let Some(p) = &mut self.progress {
            p.set_value(value);
            self.dirty_progress = true;
        }
    }

    pub fn set_progress_indeterminate(&mut self) {
        if let Some(p) = &mut self.progress {
            p.set_indeterminate();
            self.dirty_progress = true;
        }
    }

    pub fn handle_resized(&mut self, size: PhysicalSize<u32>) {
        // Skip if the size hasn't actually changed – the WM often sends a
        // confirmatory Resized right after mapping with the same dimensions.
        if let Some(ref pixmap) = self.pixmap {
            if pixmap.width() == size.width && pixmap.height() == size.height {
                return;
            }
        }
        self.pixmap = None; // force re-allocation
        self.dirty_full = true;
    }

    pub fn handle_scale_factor_changed(&mut self, scale_factor: f64) {
        self.scale_factor = scale_factor;
        self.layout_buttons();
        self.pixmap = None;
        self.dirty_full = true;
    }

    pub fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        let lx = (position.x / self.scale_factor) as f32;
        let ly = (position.y / self.scale_factor) as f32;

        let mut any_hovered = false;
        for btn in &mut self.buttons {
            let was_hovered = btn.hovered;
            btn.set_hovered(btn.hit_test(lx, ly));
            if btn.hovered {
                any_hovered = true;
            }
            if btn.hovered != was_hovered {
                self.dirty_buttons = true;
            }
        }

        // Suppress focus ring while hovering so only one button is highlighted
        if let Some(fi) = self.focused_index {
            if any_hovered && self.buttons[fi].focused {
                self.buttons[fi].set_focused(false);
                self.dirty_buttons = true;
            } else if !any_hovered && !self.buttons[fi].focused {
                self.buttons[fi].set_focused(true);
                self.dirty_buttons = true;
            }
        }
    }

    pub fn handle_mouse_pressed(&mut self) {
        let mut pressed_vec_idx = None;
        for (vec_idx, btn) in self.buttons.iter_mut().enumerate() {
            if btn.hovered {
                btn.set_pressed(true);
                pressed_vec_idx = Some(vec_idx);
                self.dirty_buttons = true;
            }
        }
        // Transfer focus to the pressed button
        if let Some(new_fi) = pressed_vec_idx {
            if self.focused_index != Some(new_fi) {
                if let Some(old_fi) = self.focused_index {
                    self.buttons[old_fi].set_focused(false);
                }
                self.buttons[new_fi].set_focused(true);
                self.focused_index = Some(new_fi);
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
        self.dirty_buttons = true;
        clicked_index
    }

    pub fn handle_close_requested(&mut self) {
        self.send_result(XDialogResult::WindowClosed);
    }

    pub fn handle_modifiers_changed(&mut self, modifiers: &Modifiers) {
        self.shift_held = modifiers.state().shift_key();
    }

    pub fn handle_key_pressed(&mut self, key: &Key) -> KeyAction {
        if self.buttons.is_empty() {
            if matches!(key, Key::Named(NamedKey::Escape)) {
                return KeyAction::Close;
            }
            return KeyAction::None;
        }

        match key {
            Key::Named(NamedKey::Enter | NamedKey::Space) => {
                if let Some(idx) = self.focused_index {
                    KeyAction::ActivateButton(self.buttons[idx].index)
                } else {
                    KeyAction::None
                }
            }
            Key::Named(NamedKey::Tab) => {
                if self.shift_held {
                    self.move_focus(-1);
                } else {
                    self.move_focus(1);
                }
                KeyAction::None
            }
            Key::Named(NamedKey::ArrowRight) => {
                self.move_focus(1);
                KeyAction::None
            }
            Key::Named(NamedKey::ArrowLeft) => {
                self.move_focus(-1);
                KeyAction::None
            }
            Key::Named(NamedKey::Escape) => KeyAction::Close,
            _ => KeyAction::None,
        }
    }

    fn move_focus(&mut self, delta: isize) {
        let count = self.buttons.len();
        if count == 0 {
            return;
        }

        let old_idx = self.focused_index.unwrap_or(0);
        self.buttons[old_idx].set_focused(false);

        let new_idx = ((old_idx as isize + delta).rem_euclid(count as isize)) as usize;
        self.buttons[new_idx].set_focused(true);
        self.focused_index = Some(new_idx);
        self.dirty_buttons = true;
    }

    pub fn tick(&mut self, elapsed: f32) -> bool {
        for btn in &mut self.buttons {
            if btn.tick(elapsed) {
                self.dirty_buttons = true;
            }
        }
        if let Some(p) = &mut self.progress {
            if p.tick(elapsed) {
                self.dirty_progress = true;
            }
        }
        self.needs_redraw()
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

        let mut total_btn_width: f32 = 0.0;
        let mut btn_widths = Vec::new();
        for btn in &self.buttons {
            let text_w = measure_text_width(&btn.label, &FONT_REGULAR, BODY_SIZE);
            let btn_w = text_w + (theme.button_text_padding * 2) as f32;
            btn_widths.push(btn_w);
            total_btn_width += btn_w;
        }
        total_btn_width += spacing * (self.buttons.len() as f32 - 1.0);

        let mut x = self.content_width as f32 - margin - total_btn_width;

        for (i, btn) in self.buttons.iter_mut().enumerate() {
            let btn_h = panel_h - margin * 2.0;
            btn.set_bounds(x, panel_y + margin, btn_widths[i], btn_h);
            x += btn_widths[i] + spacing;
        }
    }

    // ── Rendering ──────────────────────────────────────────────────────

    pub fn render_and_present(&mut self) {
        let phys_size = self.window.inner_size();
        let pw = phys_size.width;
        let ph = phys_size.height;
        if pw == 0 || ph == 0 {
            return;
        }

        // Ensure we have a pixmap of the right size
        let need_new_pixmap = self
            .pixmap
            .as_ref()
            .is_none_or(|p| p.width() != pw || p.height() != ph);
        if need_new_pixmap {
            self.pixmap = Pixmap::new(pw, ph);
            self.dirty_full = true;
        }

        if !self.needs_redraw() {
            // No new rendering needed, but the OS requested a
            // redraw (e.g. window expose) – re-present the cached buffer.
            self.present_surface(pw, ph);
            return;
        }

        // Compute dirty bounding box before clearing flags
        let dirty = self.dirty_rect(pw, ph);

        // Take pixmap out to avoid borrow conflicts with self
        let mut pixmap = self.pixmap.take().unwrap();

        if self.dirty_full {
            self.render_full(&mut pixmap);
        } else {
            if self.dirty_progress {
                self.render_progress(&mut pixmap);
            }
            if self.dirty_buttons {
                self.render_buttons(&mut pixmap);
            }
        }

        self.dirty_full = false;
        self.dirty_progress = false;
        self.dirty_buttons = false;

        // Convert only the dirty region from RGBA pixmap → ARGB cache
        self.update_cache(&pixmap, pw, dirty);

        // Put pixmap back
        self.pixmap = Some(pixmap);

        // Copy cache to surface and present
        self.present_surface(pw, ph);
    }

    /// Bounding box of the current dirty region in physical pixels.
    fn dirty_rect(&self, pw: u32, ph: u32) -> (u32, u32, u32, u32) {
        if self.dirty_full {
            return (0, 0, pw, ph);
        }
        let mut x1 = pw as f32;
        let mut y1 = ph as f32;
        let mut x2 = 0.0f32;
        let mut y2 = 0.0f32;

        if self.dirty_progress && self.progress_rect.w > 0.0 {
            let r = &self.progress_rect;
            x1 = x1.min(r.x);
            y1 = y1.min(r.y);
            x2 = x2.max(r.x + r.w);
            y2 = y2.max(r.y + r.h);
        }
        if self.dirty_buttons && self.button_panel_rect.w > 0.0 {
            let r = &self.button_panel_rect;
            x1 = x1.min(r.x);
            y1 = y1.min(r.y);
            x2 = x2.max(r.x + r.w);
            y2 = y2.max(r.y + r.h);
        }

        let ix = (x1.floor() as u32).min(pw);
        let iy = (y1.floor() as u32).min(ph);
        let ix2 = (x2.ceil() as u32).min(pw);
        let iy2 = (y2.ceil() as u32).min(ph);
        if ix >= ix2 || iy >= iy2 {
            return (0, 0, pw, ph); // fallback to full
        }
        (ix, iy, ix2 - ix, iy2 - iy)
    }

    /// Convert a rectangular region of the RGBA pixmap into the ARGB cache.
    fn update_cache(&mut self, pixmap: &Pixmap, pw: u32, dirty: (u32, u32, u32, u32)) {
        let total = (pw * self.window.inner_size().height) as usize;
        if self.present_cache.len() != total {
            // Size changed – convert everything
            self.present_cache.resize(total, 0);
            let src = pixmap.data();
            for (dst, chunk) in self.present_cache.iter_mut().zip(src.chunks_exact(4)) {
                *dst = pack_argb(chunk);
            }
            return;
        }

        let (dx, dy, dw, dh) = dirty;
        let src = pixmap.data();
        let pw = pw as usize;
        for row in dy as usize..(dy + dh) as usize {
            let row_off = row * pw;
            for col in dx as usize..(dx + dw) as usize {
                let i = row_off + col;
                let si = i * 4;
                self.present_cache[i] = pack_argb(&src[si..si + 4]);
            }
        }
    }

    /// Copy the cached ARGB buffer to the software surface and present.
    fn present_surface(&mut self, pw: u32, ph: u32) {
        if self.present_cache.is_empty() {
            return;
        }

        if self.last_surface_size != (pw, ph) {
            self.surface
                .resize(NonZeroU32::new(pw).unwrap(), NonZeroU32::new(ph).unwrap())
                .unwrap();
            self.last_surface_size = (pw, ph);
        }

        let mut buffer = self.surface.buffer_mut().unwrap();
        buffer.copy_from_slice(&self.present_cache);
        buffer.present().unwrap();
    }

    /// Full redraw of the entire dialog.
    fn render_full(&mut self, pixmap: &mut Pixmap) {
        let pw = pixmap.width() as f32;
        let ph = pixmap.height() as f32;
        let scale = self.scale_factor as f32;
        let theme = &self.theme;
        let gap = theme.default_content_margin as f32 * scale;
        let icon_size = theme.main_icon_size as f32 * scale;
        let prog_h = 6.0 * scale;

        let mut text_x = gap;
        if self.has_icon {
            text_x = gap + icon_size + gap;
        }
        let text_w = pw - text_x - gap;

        // 1. Background
        fill_rect(&mut pixmap.as_mut(), 0.0, 0.0, pw, ph, theme.color_background);

        let mut y = gap;

        // 2. Icon
        if self.has_icon {
            icons::draw_icon(&mut pixmap.as_mut(), &self.options.icon, gap, y, icon_size);
        }

        // 3. Title
        if !self.options.main_instruction.is_empty() {
            let title_layout =
                layout_text(&self.options.main_instruction, &FONT_BOLD, TITLE_SIZE * scale, text_w);
            render_text(
                &mut pixmap.as_mut(), &title_layout, &FONT_BOLD,
                TITLE_SIZE * scale, theme.color_title_text, text_x, y,
            );
            y += title_layout.total_height + gap;
        }

        // 4. Progress bar – draw and cache rect
        if self.progress.is_some() {
            self.progress_rect = PhysRect { x: text_x, y, w: text_w, h: prog_h };
            self.render_progress(pixmap);
            y += prog_h + gap;
        }

        // 5. Body text
        if !self.options.message.is_empty() {
            let body_layout =
                layout_text(&self.options.message, &FONT_REGULAR, BODY_SIZE * scale, text_w);
            render_text(
                &mut pixmap.as_mut(), &body_layout, &FONT_REGULAR,
                BODY_SIZE * scale, theme.color_body_text, text_x, y,
            );
        }

        // 6. Button panel – draw and cache rect
        if !self.buttons.is_empty() {
            let panel_y = (self.content_height - theme.button_panel_height) as f32 * scale;
            let panel_h = theme.button_panel_height as f32 * scale;
            self.button_panel_rect = PhysRect { x: 0.0, y: panel_y, w: pw, h: panel_h };
            fill_rect(&mut pixmap.as_mut(), 0.0, panel_y, pw, panel_h, theme.color_background_alt);
            self.render_buttons(pixmap);
        }
    }

    /// Redraw only the progress bar region on the existing pixmap.
    fn render_progress(&self, pixmap: &mut Pixmap) {
        let Some(ref progress) = self.progress else { return };
        let r = self.progress_rect;
        let scale = self.scale_factor as f32;

        // Clear the progress rect with background
        fill_rect(&mut pixmap.as_mut(), r.x, r.y, r.w, r.h, self.theme.color_background);

        // Background track
        fill_rounded_rect(
            &mut pixmap.as_mut(), r.x, r.y, r.w, r.h,
            2.0 * scale, self.theme.color_progress_background,
        );

        // Foreground bar
        let bar_start = progress.state.x1 * r.w;
        let bar_end = progress.state.x2 * r.w;
        let bar_w = bar_end - bar_start;
        if bar_w > 0.0 {
            fill_rounded_rect(
                &mut pixmap.as_mut(), r.x + bar_start, r.y, bar_w, r.h,
                2.0 * scale, self.theme.color_progress_foreground,
            );
        }
    }

    /// Redraw only the buttons on the existing pixmap.
    fn render_buttons(&self, pixmap: &mut Pixmap) {
        let scale = self.scale_factor as f32;
        let r = self.button_panel_rect;

        // Clear button panel with its background color
        if r.w > 0.0 {
            fill_rect(&mut pixmap.as_mut(), r.x, r.y, r.w, r.h, self.theme.color_background_alt);
        }

        for btn in &self.buttons {
            let colors = btn.current_colors();
            let bx = btn.x * scale;
            let by = btn.y * scale;
            let bw = btn.width * scale;
            let bh = btn.height * scale;
            let radius = colors.border_radius as f32 * scale;

            fill_rounded_rect(
                &mut pixmap.as_mut(), bx, by, bw, bh, radius,
                (colors.fill_r, colors.fill_g, colors.fill_b),
            );

            if colors.border_width > 0 {
                stroke_rounded_rect(
                    &mut pixmap.as_mut(), bx, by, bw, bh, radius,
                    (colors.border_r, colors.border_g, colors.border_b),
                    colors.border_width as f32 * scale,
                );
            }

            let label_layout =
                layout_text(&btn.label, &FONT_REGULAR, BODY_SIZE * scale, bw);
            let text_x = bx + (bw - label_layout.total_width) / 2.0;
            let text_y = by + (bh - label_layout.total_height) / 2.0;
            render_text(
                &mut pixmap.as_mut(), &label_layout, &FONT_REGULAR,
                BODY_SIZE * scale, (colors.text_r, colors.text_g, colors.text_b),
                text_x, text_y,
            );
        }
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

    let pad_x = if has_icon {
        gap + theme.main_icon_size + gap + gap
    } else {
        gap + gap
    };

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

    let text_w = (final_width - pad_x) as f32;
    let wrapped_title = layout_text(&options.main_instruction, &FONT_BOLD, TITLE_SIZE, text_w);
    let wrapped_body = layout_text(&options.message, &FONT_REGULAR, BODY_SIZE, text_w);

    let mut h = gap;
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
    if has_icon {
        h = h.max(gap + theme.main_icon_size + gap);
    }
    if !options.buttons.is_empty() {
        h += theme.button_panel_height;
    }

    (final_width, h)
}

/// Pack RGBA bytes into a single ARGB u32 for the software surface.
#[inline(always)]
fn pack_argb(rgba: &[u8]) -> u32 {
    (rgba[3] as u32) << 24 | (rgba[0] as u32) << 16 | (rgba[1] as u32) << 8 | rgba[2] as u32
}
