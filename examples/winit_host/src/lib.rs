//! The glue between a **winit 0.29** event loop and `xdialog::host` (used by `main.rs` and
//! `tests/host.rs`). Copy it into your application and adapt it: nothing here is specific to this
//! example.
//!
//! - [`XdWindows`]: the windows the host created on xdialog's behalf, by [`WindowKey`] and by
//!   winit [`WindowId`] (to route window events).
//! - [`Glue`]: implements [`HostWindows`] over a borrowed event-loop target and the table.
//! - [`translate`]: winit 0.29 `WindowEvent` -> `xdialog::host::HostEvent`.
//!
//! Loop integration (see `main.rs`):
//! - `Event::WindowEvent` for a window in [`XdWindows::by_id`]: `RedrawRequested` ->
//!   `xdialog::host::redraw`, everything else -> [`translate`] -> `xdialog::host::handle_event`.
//! - `Event::AboutToWait`: `xdialog::host::pump`, then `ControlFlow::WaitUntil(deadline)` / `Wait`.
//! - `Event::LoopExiting`: `xdialog::host::shutdown` (required).

use std::collections::HashMap;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton as WMouseButton, MouseScrollDelta, TouchPhase as WTouchPhase, WindowEvent};
use winit::event_loop::EventLoopWindowTarget;
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::{Theme, Window, WindowBuilder, WindowButtons, WindowId};
pub use winit029 as winit;
use xdialog::host::{HostEvent, HostWindow, HostWindows, Key, Modifiers, MouseButton, ScrollDelta, TouchPhase, WindowKey, WindowRequest};

/// The host's bookkeeping for windows it created on behalf of xdialog.
#[derive(Default)]
pub struct XdWindows {
    pub by_key: HashMap<WindowKey, Window>,
    pub by_id: HashMap<WindowId, WindowKey>,
    /// Owner for dialog windows (Windows: keeps them above the main window).
    pub owner: Option<raw_window_handle::RawWindowHandle>,
}

/// A borrow of the event-loop target and the window table, handed to xdialog for one call.
pub struct Glue<'a, T: 'static> {
    /// The event-loop target (winit 0.29 `EventLoopWindowTarget`; 0.30: `ActiveEventLoop`).
    pub elwt: &'a EventLoopWindowTarget<T>,
    /// The window table.
    pub windows: &'a mut XdWindows,
}

// SAFETY: windows are dropped only in `destroy_window`, so their handles stay valid until then, and
// the display (owned by the event loop) outlives them. The loop must call `xdialog::host::shutdown`
// on `Event::LoopExiting` (as `main.rs` and `tests/host.rs` do), before `XdWindows` and the event
// loop are dropped.
unsafe impl<T: 'static> HostWindows for Glue<'_, T> {
    fn create_window(&mut self, req: &WindowRequest) -> Result<HostWindow, String> {
        let mut builder = WindowBuilder::new().with_title(req.title.clone())
                                              .with_inner_size(LogicalSize::new(req.width, req.height))
                                              .with_resizable(req.resizable)
                                              .with_enabled_buttons(WindowButtons::CLOSE)
                                              .with_visible(req.visible)
                                              .with_active(req.active)
                                              .with_theme(if req.follow_system { None } else { Some(if req.dark { Theme::Dark } else { Theme::Light }) });
        // Centre on the primary monitor unless xdialog suggests a position.
        let position = req.position.map(|(x, y)| PhysicalPosition::new(x, y)).or_else(|| {
                                                                                   let m = self.elwt.primary_monitor()?;
                                                                                   let s = m.scale_factor();
                                                                                   let (mp, ms) = (m.position(), m.size());
                                                                                   Some(PhysicalPosition::new(mp.x + ((ms.width as f64 - req.width * s) / 2.0) as i32,
                                                                                                              mp.y + ((ms.height as f64 - req.height * s) / 2.0) as i32))
                                                                               });
        if let Some(p) = position {
            builder = builder.with_position(p);
        }
        #[cfg(windows)]
        if let Some(raw_window_handle::RawWindowHandle::Win32(h)) = self.windows.owner {
            use winit::platform::windows::WindowBuilderExtWindows;
            builder = builder.with_owner_window(h.hwnd.get());
        }
        let window = builder.build(self.elwt).map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let mut hw = HostWindow::new(window.display_handle().map_err(|e| e.to_string())?.as_raw(),
                                     window.window_handle().map_err(|e| e.to_string())?.as_raw(),
                                     window.scale_factor(),
                                     (size.width, size.height));
        // winit 0.29 has no work-area query; the monitor height is a good enough cap.
        if let Some(m) = window.current_monitor() {
            hw = hw.with_work_area_height(m.size().height as f64 / m.scale_factor());
        }
        self.windows.by_id.insert(window.id(), req.key);
        self.windows.by_key.insert(req.key, window);
        Ok(hw)
    }

    fn set_visible(&mut self, key: WindowKey, visible: bool) {
        if let Some(w) = self.windows.by_key.get(&key) {
            w.set_visible(visible);
        }
    }

    fn set_inner_size(&mut self, key: WindowKey, width: f64, height: f64) {
        if let Some(w) = self.windows.by_key.get(&key) {
            let _ = w.request_inner_size(LogicalSize::new(width, height));
        }
    }

    fn request_redraw(&mut self, key: WindowKey) {
        if let Some(w) = self.windows.by_key.get(&key) {
            w.request_redraw();
        }
    }

    fn set_dark(&mut self, key: WindowKey, dark: bool) {
        if let Some(w) = self.windows.by_key.get(&key) {
            w.set_theme(Some(if dark { Theme::Dark } else { Theme::Light }));
        }
    }

    fn destroy_window(&mut self, key: WindowKey) {
        if let Some(w) = self.windows.by_key.remove(&key) {
            self.windows.by_id.remove(&w.id());
            // Dropping `w` destroys the native window.
        }
    }
}

/// Translate a winit 0.29 window event into xdialog's version-neutral event.
pub fn translate(event: &WindowEvent) -> Option<HostEvent> {
    Some(match event {
        WindowEvent::Resized(s) => HostEvent::Resized { width: s.width, height: s.height },
        WindowEvent::ScaleFactorChanged { scale_factor, .. } => HostEvent::ScaleFactorChanged { scale_factor: *scale_factor },
        WindowEvent::CursorMoved { position, .. } => HostEvent::CursorMoved { x: position.x, y: position.y },
        WindowEvent::CursorLeft { .. } => HostEvent::CursorLeft,
        WindowEvent::MouseInput { state, button, .. } => HostEvent::MouseButton { button: match button {
                                                                                     WMouseButton::Left => MouseButton::Primary,
                                                                                     WMouseButton::Right => MouseButton::Secondary,
                                                                                     WMouseButton::Middle => MouseButton::Middle,
                                                                                     _ => MouseButton::Other,
                                                                                 },
                                                                                 pressed: *state == ElementState::Pressed },
        WindowEvent::MouseWheel { delta, .. } => HostEvent::MouseWheel { delta: match delta {
                                                     MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines { x: *x, y: *y },
                                                     MouseScrollDelta::PixelDelta(p) => ScrollDelta::Pixels { x: p.x, y: p.y },
                                                 } },
        WindowEvent::Touch(t) => HostEvent::Touch { id: t.id,
                                                    phase: match t.phase {
                                                        WTouchPhase::Started => TouchPhase::Started,
                                                        WTouchPhase::Moved => TouchPhase::Moved,
                                                        WTouchPhase::Ended => TouchPhase::Ended,
                                                        WTouchPhase::Cancelled => TouchPhase::Cancelled,
                                                    },
                                                    x: t.location.x,
                                                    y: t.location.y },
        WindowEvent::ModifiersChanged(m) => {
            let s = m.state();
            HostEvent::Modifiers(Modifiers::new(s.shift_key(), s.control_key(), s.alt_key(), s.super_key()))
        }
        // Synthetic key events (focus changes) must not be forwarded.
        WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
            let key = match &event.logical_key {
                WKey::Named(NamedKey::Enter) => Key::Enter,
                WKey::Named(NamedKey::Escape) => Key::Escape,
                WKey::Named(NamedKey::Tab) => Key::Tab,
                WKey::Named(NamedKey::Space) => Key::Space,
                WKey::Named(NamedKey::ArrowLeft) => Key::ArrowLeft,
                WKey::Named(NamedKey::ArrowRight) => Key::ArrowRight,
                WKey::Named(NamedKey::ArrowUp) => Key::ArrowUp,
                WKey::Named(NamedKey::ArrowDown) => Key::ArrowDown,
                WKey::Named(NamedKey::Home) => Key::Home,
                WKey::Named(NamedKey::End) => Key::End,
                WKey::Named(NamedKey::PageUp) => Key::PageUp,
                WKey::Named(NamedKey::PageDown) => Key::PageDown,
                _ => return None,
            };
            HostEvent::Key { key, pressed: event.state == ElementState::Pressed, repeat: event.repeat }
        }
        WindowEvent::Focused(f) => HostEvent::Focused(*f),
        WindowEvent::ThemeChanged(_) => HostEvent::ThemeChanged,
        WindowEvent::CloseRequested => HostEvent::CloseRequested,
        _ => return None,
    })
}
