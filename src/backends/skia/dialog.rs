use std::num::NonZeroU32;
use std::sync::Arc;

use softbuffer::Surface;
use tiny_skia::Pixmap;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::Modifiers;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes};

use crate::model::*;
use crate::{ProgressButtonCallback, ProgressDialogProxy};

use super::background::{Background, Footer};
use super::button::SkiaButton;
use super::component::{Component, ControllerUpdate, LayoutCtx, PaintCtx, Rect, Role};
use super::icon::Icon;
use super::label::{Label, LabelKind};
use super::progress::SkiaProgressBar;
use super::theme::SkiaTheme;

pub enum KeyAction {
    None,
    ActivateButton(usize),
    Close,
}

const MIN_WIDTH: f32 = 350.0;
const MAX_WIDTH: f32 = 600.0;

/// A dialog window. Owns its components in paint/z-order and a single persistent pixmap; each
/// frame paints the dirty components onto the pixmap and converts the whole thing to the OS buffer.
pub struct SkiaDialog {
    pub window: Arc<Window>,
    surface: Surface<Arc<Window>, Arc<Window>>,
    /// Single persistent internal RGBA buffer; converted to the softbuffer ARGB surface at present.
    pixmap: Option<Pixmap>,
    theme: SkiaTheme,
    /// Components in z-order: Background, Footer?, Icon?, Title?, Progress?, Body?, Button(s).
    components: Vec<Box<dyn Component>>,
    /// Index (into `components`) of the focused component, if any.
    focused: Option<usize>,
    shift_held: bool,
    result_sender: Option<oneshot::Sender<XDialogResult>>,
    button_callback: Option<ProgressButtonCallback>,
    scale_factor: f64,
    /// Forces every component to repaint next frame (resize / relayout / pixmap realloc).
    repaint_all: bool,
    /// Last physical size passed to `surface.resize()`.
    last_surface_size: (u32, u32),
}

impl SkiaDialog {
    pub fn new(
        event_loop: &ActiveEventLoop,
        options: XDialogOptions,
        theme: &SkiaTheme,
        has_progress: bool,
        result_sender: oneshot::Sender<XDialogResult>,
        button_callback: Option<ProgressButtonCallback>,
    ) -> Self {
        // Build components in paint/z-order.
        let mut components: Vec<Box<dyn Component>> = Vec::new();
        components.push(Box::new(Background::new()));

        if !options.buttons.is_empty() {
            components.push(Box::new(Footer::new()));
        }
        if options.icon != XDialogIcon::None {
            components.push(Box::new(Icon::new(options.icon.clone())));
        }
        if !options.main_instruction.is_empty() {
            components.push(Box::new(Label::new(LabelKind::Title, &options.main_instruction)));
        }
        if has_progress {
            components.push(Box::new(SkiaProgressBar::new()));
        }
        if !options.message.is_empty() {
            components.push(Box::new(Label::new(LabelKind::Body, &options.message)));
        }

        // Buttons, honouring the theme's button order.
        let button_iter: Vec<(usize, &String)> = if theme.button_order_reversed {
            options.buttons.iter().enumerate().rev().collect()
        } else {
            options.buttons.iter().enumerate().collect()
        };
        for (index, label) in button_iter {
            components.push(Box::new(SkiaButton::new(label, index, theme)));
        }

        // Compute the initial (logical, scale-independent) window size for the window attributes.
        let (win_w, win_h) = layout_components(&mut components, theme);

        let mut attrs = WindowAttributes::default()
            .with_title(options.title.clone())
            .with_inner_size(LogicalSize::new(win_w as f64, win_h as f64))
            .with_resizable(false);

        // Pre-compute the centered position so the WM doesn't need to reposition after mapping,
        // which causes a visible jitter.
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

        let mut dialog = Self {
            window,
            surface,
            pixmap: None,
            theme: theme.clone(),
            components,
            focused: None,
            shift_held: false,
            result_sender: Some(result_sender),
            button_callback,
            scale_factor,
            repaint_all: true,
            last_surface_size: (0, 0),
        };

        // Focus the last focusable component (button) by default.
        if let Some(idx) = dialog.last_focusable() {
            dialog.focused = Some(idx);
            dialog.components[idx].set_focused(true);
        }

        // Pre-render the first frame synchronously so the window has content before the
        // compositor/WM ever displays it.
        dialog.render_and_present();
        dialog
    }

    fn last_focusable(&self) -> Option<usize> {
        self.components
            .iter()
            .enumerate()
            .rev()
            .find(|(_, c)| c.focusable())
            .map(|(i, _)| i)
    }

    pub fn needs_redraw(&self) -> bool {
        self.repaint_all || self.components.iter().any(|c| c.is_dirty())
    }

    pub fn is_animating(&self) -> bool {
        self.components.iter().any(|c| c.is_animating())
    }

    pub fn send_result(&mut self, result: XDialogResult) {
        if let Some(sender) = self.result_sender.take() {
            let _ = sender.send(result);
        }
    }

    /// Handle a button click on this dialog. If a progress button callback is registered, invoke
    /// it with a non-owning proxy and return whether the dialog should stay open. Otherwise deliver
    /// the click as a `ButtonPressed` result and return `false` (the dialog should close).
    pub fn on_button_clicked(&mut self, id: usize, index: usize) -> bool {
        if let Some(cb) = self.button_callback.as_mut() {
            let proxy = ProgressDialogProxy::non_owning(id);
            (cb.0)(index, &proxy)
        } else {
            self.send_result(XDialogResult::ButtonPressed(index));
            false
        }
    }

    pub fn close(&mut self) {
        self.send_result(XDialogResult::WindowClosed);
        self.window.set_visible(false);
    }

    pub fn set_body_text(&mut self, text: &str) {
        if self.broadcast(&ControllerUpdate::BodyText(text)) {
            self.layout();
        }
    }

    pub fn set_progress_value(&mut self, value: f32) {
        if self.broadcast(&ControllerUpdate::ProgressValue(value)) {
            self.layout();
        }
    }

    pub fn set_progress_indeterminate(&mut self) {
        if self.broadcast(&ControllerUpdate::ProgressIndeterminate) {
            self.layout();
        }
    }

    /// Broadcast a controller update to every component. Returns whether any component reported a
    /// size change that requires a relayout.
    fn broadcast(&mut self, update: &ControllerUpdate) -> bool {
        let mut relayout = false;
        for c in self.components.iter_mut() {
            if c.apply(update) {
                relayout = true;
            }
        }
        relayout
    }

    pub fn handle_resized(&mut self, size: PhysicalSize<u32>) {
        // Skip if the size hasn't actually changed – the WM often sends a confirmatory Resized
        // right after mapping with the same dimensions.
        if let Some(ref pixmap) = self.pixmap {
            if pixmap.width() == size.width && pixmap.height() == size.height {
                return;
            }
        }
        self.pixmap = None; // force re-allocation
        self.repaint_all = true;
    }

    pub fn handle_scale_factor_changed(&mut self, scale_factor: f64) {
        // Layout is logical (scale-independent); only the physical pixmap needs to grow/shrink.
        self.scale_factor = scale_factor;
        self.pixmap = None;
        self.repaint_all = true;
    }

    pub fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        let lx = (position.x / self.scale_factor) as f32;
        let ly = (position.y / self.scale_factor) as f32;

        let mut any_hovered = false;
        for c in self.components.iter_mut() {
            c.set_hovered(c.bounds().contains(lx, ly));
            if c.is_hovered() {
                any_hovered = true;
            }
        }

        // Suppress the focus ring while hovering, so only one button is highlighted.
        if let Some(fi) = self.focused {
            self.components[fi].set_focused(!any_hovered);
        }
    }

    pub fn handle_mouse_pressed(&mut self) {
        let mut pressed_idx = None;
        for (i, c) in self.components.iter_mut().enumerate() {
            if c.is_hovered() {
                c.set_pressed(true);
                pressed_idx = Some(i);
            }
        }
        // Transfer focus to the pressed component.
        if let Some(new_fi) = pressed_idx {
            if self.focused != Some(new_fi) {
                if let Some(old_fi) = self.focused {
                    self.components[old_fi].set_focused(false);
                }
                self.components[new_fi].set_focused(true);
                self.focused = Some(new_fi);
            }
        }
    }

    pub fn handle_mouse_released(&mut self) -> Option<usize> {
        let mut clicked = None;
        for c in self.components.iter_mut() {
            if c.is_pressed() && c.is_hovered() {
                clicked = c.activation_index();
            }
            c.set_pressed(false);
        }
        clicked
    }

    pub fn handle_close_requested(&mut self) {
        self.send_result(XDialogResult::WindowClosed);
    }

    pub fn handle_modifiers_changed(&mut self, modifiers: &Modifiers) {
        self.shift_held = modifiers.state().shift_key();
    }

    pub fn handle_key_pressed(&mut self, key: &Key) -> KeyAction {
        let has_focusable = self.components.iter().any(|c| c.focusable());
        if !has_focusable {
            if matches!(key, Key::Named(NamedKey::Escape)) {
                return KeyAction::Close;
            }
            return KeyAction::None;
        }

        match key {
            Key::Named(NamedKey::Enter | NamedKey::Space) => {
                if let Some(idx) = self.focused {
                    if let Some(ai) = self.components[idx].activation_index() {
                        return KeyAction::ActivateButton(ai);
                    }
                }
                KeyAction::None
            }
            Key::Named(NamedKey::Tab) => {
                self.move_focus(if self.shift_held { -1 } else { 1 });
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

    /// Move keyboard focus among the focusable components by `delta` (wrapping).
    fn move_focus(&mut self, delta: isize) {
        let focusable: Vec<usize> = self
            .components
            .iter()
            .enumerate()
            .filter(|(_, c)| c.focusable())
            .map(|(i, _)| i)
            .collect();
        if focusable.is_empty() {
            return;
        }

        let cur_pos = self
            .focused
            .and_then(|fi| focusable.iter().position(|&i| i == fi))
            .unwrap_or(0);

        if let Some(old) = self.focused {
            self.components[old].set_focused(false);
        }

        let n = focusable.len() as isize;
        let new_pos = (cur_pos as isize + delta).rem_euclid(n) as usize;
        let new_fi = focusable[new_pos];
        self.components[new_fi].set_focused(true);
        self.focused = Some(new_fi);
    }

    pub fn tick(&mut self, elapsed: f32) -> bool {
        for c in self.components.iter_mut() {
            c.tick(elapsed);
        }
        self.needs_redraw()
    }

    /// Re-run layout (after body-text changes alter the content height) and request the new size.
    fn layout(&mut self) {
        let (win_w, win_h) = layout_components(&mut self.components, &self.theme);
        let _ = self
            .window
            .request_inner_size(LogicalSize::new(win_w as f64, win_h as f64));
        self.repaint_all = true;
    }

    // ── Rendering ──────────────────────────────────────────────────────

    pub fn render_and_present(&mut self) {
        let phys_size = self.window.inner_size();
        let pw = phys_size.width;
        let ph = phys_size.height;
        if pw == 0 || ph == 0 {
            return;
        }

        // Ensure we have a pixmap matching the physical window size.
        let need_new_pixmap = self
            .pixmap
            .as_ref()
            .is_none_or(|p| p.width() != pw || p.height() != ph);
        if need_new_pixmap {
            self.pixmap = Pixmap::new(pw, ph);
            self.repaint_all = true;
        }

        if !self.needs_redraw() {
            // Nothing changed, but the OS requested a redraw (e.g. window expose) – re-present the
            // existing pixmap.
            self.present(pw, ph);
            return;
        }

        // Take the pixmap out so the paint loop can borrow `&mut components` and `&theme` disjointly.
        let mut pixmap = self.pixmap.take().unwrap();
        {
            let ctx = PaintCtx {
                theme: &self.theme,
                scale: self.scale_factor as f32,
            };
            let repaint_all = self.repaint_all;
            let mut pm = pixmap.as_mut();
            for c in self.components.iter_mut() {
                if repaint_all || c.is_dirty() {
                    c.paint(&mut pm, &ctx);
                }
            }
        }
        self.repaint_all = false;
        self.pixmap = Some(pixmap);

        self.present(pw, ph);
    }

    /// Convert the internal RGBA pixmap to the softbuffer ARGB surface and present it.
    fn present(&mut self, pw: u32, ph: u32) {
        let Some(ref pixmap) = self.pixmap else {
            return;
        };

        if self.last_surface_size != (pw, ph) {
            self.surface
                .resize(NonZeroU32::new(pw).unwrap(), NonZeroU32::new(ph).unwrap())
                .unwrap();
            self.last_surface_size = (pw, ph);
        }

        let mut buffer = self.surface.buffer_mut().unwrap();
        crate::pixels::rgba_to_argb(pixmap.data(), &mut buffer);
        buffer.present().unwrap();
    }
}

/// Lay out all components in **logical** pixels and return the logical window `(width, height)`.
///
/// Positions are resolution-independent: components store logical bounds and scale by
/// `PaintCtx::scale` when painting. Generic over [`Role`], so a new content component slots into
/// the vertical stack automatically.
fn layout_components(components: &mut [Box<dyn Component>], theme: &SkiaTheme) -> (f32, f32) {
    let gap = theme.default_content_margin as f32;
    let icon_size = theme.main_icon_size as f32;

    let has_icon = components.iter().any(|c| c.role() == Role::Icon);
    let has_buttons = components.iter().any(|c| c.role() == Role::Button);

    // 1. Natural content width: measure Content components at "infinite" width (no wrap).
    let mut natural_w: f32 = 0.0;
    {
        let ctx = LayoutCtx {
            theme,
            available_width: f32::INFINITY,
        };
        for c in components.iter_mut() {
            if c.role() == Role::Content {
                natural_w = natural_w.max(c.measure(&ctx).w);
            }
        }
    }

    // 2. Clamp to the window content width.
    let final_width = clamp_window_width(natural_w);
    let text_x = if has_icon { gap + icon_size + gap } else { gap };
    let text_w = final_width - text_x - gap;

    // 3. Measure Content at the clamped width and stack vertically with `gap` between items.
    let mut y = gap;
    {
        let ctx = LayoutCtx {
            theme,
            available_width: text_w,
        };
        for c in components.iter_mut() {
            if c.role() == Role::Content {
                let h = c.measure(&ctx).h;
                c.set_bounds(Rect::new(text_x, y, text_w, h));
                y += h + gap;
            }
        }
    }

    // 4. The content region must be at least tall enough for the icon.
    let mut content_region_h = y;
    if has_icon {
        content_region_h = content_region_h.max(gap + icon_size + gap);
    }
    let win_w = final_width;
    let win_h = content_region_h + if has_buttons { theme.button_panel_height as f32 } else { 0.0 };

    // 5. Position the background, icon and footer by role.
    for c in components.iter_mut() {
        match c.role() {
            Role::Background => c.set_bounds(Rect::new(0.0, 0.0, win_w, win_h)),
            Role::Icon => c.set_bounds(Rect::new(gap, gap, icon_size, icon_size)),
            Role::Footer => c.set_bounds(Rect::new(
                0.0,
                content_region_h,
                win_w,
                theme.button_panel_height as f32,
            )),
            _ => {}
        }
    }

    // 6. Lay out the button row, right-aligned within the footer.
    layout_button_row(components, theme, win_w, content_region_h, text_w);

    (win_w, win_h)
}

/// Right-align the button row within the footer strip whose top is at `panel_y`.
fn layout_button_row(
    components: &mut [Box<dyn Component>],
    theme: &SkiaTheme,
    win_w: f32,
    panel_y: f32,
    text_w: f32,
) {
    let panel_h = theme.button_panel_height as f32;
    let margin = theme.button_panel_margin as f32;
    let spacing = theme.button_panel_spacing as f32;

    let mut indices: Vec<usize> = Vec::new();
    let mut widths: Vec<f32> = Vec::new();
    let mut total: f32 = 0.0;
    {
        let ctx = LayoutCtx {
            theme,
            available_width: text_w,
        };
        for (i, c) in components.iter_mut().enumerate() {
            if c.role() == Role::Button {
                let w = c.measure(&ctx).w;
                indices.push(i);
                widths.push(w);
                total += w;
            }
        }
    }
    if indices.is_empty() {
        return;
    }
    total += spacing * (indices.len() as f32 - 1.0);

    let btn_h = panel_h - margin * 2.0;
    let mut x = win_w - margin - total;
    for (k, &ci) in indices.iter().enumerate() {
        components[ci].set_bounds(Rect::new(x, panel_y + margin, widths[k], btn_h));
        x += widths[k] + spacing;
    }
}

/// Map the natural (unwrapped) content width to a clamped window content width — the same curve as
/// the original `compute_window_size`.
fn clamp_window_width(natural_w: f32) -> f32 {
    let window_width = if natural_w <= 600.0 {
        300.0
    } else if natural_w >= 4000.0 {
        600.0
    } else {
        300.0 + ((natural_w - 600.0) / 3400.0) * 300.0
    };
    window_width.clamp(MIN_WIDTH, MAX_WIDTH)
}
