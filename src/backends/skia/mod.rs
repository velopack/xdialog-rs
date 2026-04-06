mod button;
mod dialog;
mod font;
mod icons;
mod progress;
mod renderer;
mod text;
mod theme;

use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use dialog::KeyAction;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopBuilder};
use winit::platform::x11::EventLoopBuilderExtX11;
use winit::platform::wayland::EventLoopBuilderExtWayland;
use winit::window::WindowId;

use crate::model::*;

use super::XDialogBackendImpl;

pub struct SkiaBackend;

struct AppState {
    theme: theme::SkiaTheme,
    dialogs: HashMap<usize, dialog::SkiaDialog>,
    window_to_id: HashMap<WindowId, usize>,
    current_time: Instant,
}

impl AppState {
    fn new() -> Self {
        Self {
            theme: theme::get_theme(),
            dialogs: HashMap::new(),
            window_to_id: HashMap::new(),
            current_time: Instant::now(),
        }
    }

    fn handle_message(&mut self, event_loop: &ActiveEventLoop, msg: DialogMessageRequest) {
        match msg {
            DialogMessageRequest::None => {}
            DialogMessageRequest::ExitEventLoop => {
                for (_, mut d) in self.dialogs.drain() {
                    d.close();
                }
                self.window_to_id.clear();
                event_loop.exit();
            }
            DialogMessageRequest::CloseWindow(id) => {
                if let Some(mut d) = self.dialogs.remove(&id) {
                    self.window_to_id.remove(&d.window.id());
                    d.close();
                }
            }
            DialogMessageRequest::ShowMessageWindow(id, data, creation) => {
                let (sender, receiver) = oneshot::channel();
                let d = dialog::SkiaDialog::new(event_loop, data, &self.theme, false, sender);
                self.window_to_id.insert(d.window.id(), id);
                self.dialogs.insert(id, d);
                let _ = creation.send(Ok(receiver));
            }
            DialogMessageRequest::ShowProgressWindow(id, data, creation) => {
                let (sender, receiver) = oneshot::channel();
                let d = dialog::SkiaDialog::new(event_loop, data, &self.theme, true, sender);
                self.window_to_id.insert(d.window.id(), id);
                self.dialogs.insert(id, d);
                let _ = creation.send(Ok(receiver));
            }
            DialogMessageRequest::SetProgressIndeterminate(id) => {
                if let Some(d) = self.dialogs.get_mut(&id) {
                    d.set_progress_indeterminate();
                }
            }
            DialogMessageRequest::SetProgressValue(id, value) => {
                if let Some(d) = self.dialogs.get_mut(&id) {
                    d.set_progress_value(value);
                }
            }
            DialogMessageRequest::SetProgressText(id, text) => {
                if let Some(d) = self.dialogs.get_mut(&id) {
                    d.set_body_text(&text);
                }
            }
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.current_time);
        // Skip near-zero ticks to prevent double-render per frame:
        // about_to_wait is called again right after RedrawRequested,
        // and a sub-ms tick would just set dirty flags again wastefully.
        if elapsed < Duration::from_millis(4) {
            return;
        }
        self.current_time = now;
        let elapsed_secs = elapsed.as_secs_f32();
        for dialog in self.dialogs.values_mut() {
            dialog.tick(elapsed_secs);
        }
    }
}

impl ApplicationHandler<DialogMessageRequest> for AppState {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: DialogMessageRequest) {
        self.handle_message(event_loop, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.tick();

        let mut any_needs_frame = false;
        for dialog in self.dialogs.values() {
            if dialog.needs_redraw() {
                dialog.window.request_redraw();
            }
            if dialog.needs_redraw() || dialog.is_animating() {
                any_needs_frame = true;
            }
        }

        if any_needs_frame {
            // Cap animation rendering at ~60fps
            const FRAME_TIME: Duration = Duration::from_millis(16);
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + FRAME_TIME));
        } else {
            // Nothing animating – sleep until next event
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(&dialog_id) = self.window_to_id.get(&window_id) else {
            return;
        };
        let Some(dialog) = self.dialogs.get_mut(&dialog_id) else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => {
                dialog.handle_close_requested();
                let wid = dialog.window.id();
                self.dialogs.remove(&dialog_id);
                self.window_to_id.remove(&wid);
            }
            WindowEvent::RedrawRequested => {
                dialog.render_and_present();
            }
            WindowEvent::Resized(size) => {
                dialog.handle_resized(size);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                dialog.handle_scale_factor_changed(scale_factor);
            }
            WindowEvent::CursorMoved { position, .. } => {
                dialog.handle_cursor_moved(position);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left {
                    match state {
                        ElementState::Pressed => {
                            dialog.handle_mouse_pressed();
                        }
                        ElementState::Released => {
                            if let Some(index) = dialog.handle_mouse_released() {
                                dialog.send_result(XDialogResult::ButtonPressed(index));
                                dialog.window.set_visible(false);
                                let wid = window_id;
                                self.dialogs.remove(&dialog_id);
                                self.window_to_id.remove(&wid);
                            }
                        }
                    }
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                dialog.handle_modifiers_changed(&modifiers);
            }
            WindowEvent::KeyboardInput { event, is_synthetic, .. } => {
                if event.state == ElementState::Pressed && !is_synthetic {
                    match dialog.handle_key_pressed(&event.logical_key) {
                        KeyAction::ActivateButton(index) => {
                            if !event.repeat {
                                dialog.send_result(XDialogResult::ButtonPressed(index));
                                dialog.window.set_visible(false);
                                let wid = window_id;
                                self.dialogs.remove(&dialog_id);
                                self.window_to_id.remove(&wid);
                            }
                        }
                        KeyAction::Close => {
                            if !event.repeat {
                                dialog.handle_close_requested();
                                let wid = dialog.window.id();
                                self.dialogs.remove(&dialog_id);
                                self.window_to_id.remove(&wid);
                            }
                        }
                        KeyAction::None => {}
                    }
                }
            }
            _ => {}
        }
    }
}

impl SkiaBackend {
    /// When no display server is available, drain the receiver channel and
    /// respond to every dialog-creation request with `NoBackendAvailable`.
    fn drain_with_error(receiver: Receiver<DialogMessageRequest>) {
        while let Ok(msg) = receiver.recv() {
            match msg {
                DialogMessageRequest::ExitEventLoop => break,
                DialogMessageRequest::ShowMessageWindow(_id, _options, creation) => {
                    let _ = creation.send(Err(crate::XDialogError::NoBackendAvailable));
                }
                DialogMessageRequest::ShowProgressWindow(_id, _options, creation) => {
                    let _ = creation.send(Err(crate::XDialogError::NoBackendAvailable));
                }
                _ => {}
            }
        }
    }
}

impl XDialogBackendImpl for SkiaBackend {
    fn run_loop(receiver: Receiver<DialogMessageRequest>, _theme: XDialogTheme) {
        let mut builder = EventLoopBuilder::<DialogMessageRequest>::default();
        EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
        EventLoopBuilderExtWayland::with_any_thread(&mut builder, true);

        let event_loop = match builder.build() {
            Ok(el) => el,
            Err(e) => {
                error!("xdialog: failed to create event loop (no display server?): {}", e);
                Self::drain_with_error(receiver);
                return;
            }
        };

        let proxy = event_loop.create_proxy();

        // Forward channel messages into the winit event loop as user events
        std::thread::spawn(move || {
            while let Ok(msg) = receiver.recv() {
                if proxy.send_event(msg).is_err() {
                    break;
                }
            }
        });

        let mut state = AppState::new();
        if let Err(e) = event_loop.run_app(&mut state) {
            error!("xdialog: skia event loop error: {:?}", e);
        }
    }
}
