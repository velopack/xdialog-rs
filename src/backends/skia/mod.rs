mod background;
mod button;
mod component;
mod desktop;
mod dialog;
mod font;
mod icon;
mod icons;
#[cfg(feature = "skia-instrumentation")]
mod instrument;
mod label;
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
use winit::window::WindowId;

use crate::model::*;

use super::XDialogBackendImpl;

pub struct SkiaBackend;

struct AppState {
    theme: theme::SkiaTheme,
    dialogs: HashMap<usize, dialog::SkiaDialog>,
    window_to_id: HashMap<WindowId, usize>,
    current_time: Instant,
    /// Scheduled time of the next animation frame while something is animating (`None` when idle).
    /// Anchored to a fixed cadence so frame pacing stays even — see `about_to_wait`.
    next_frame_at: Option<Instant>,
}

impl AppState {
    fn new(theme: theme::SkiaTheme) -> Self {
        Self {
            theme,
            dialogs: HashMap::new(),
            window_to_id: HashMap::new(),
            current_time: Instant::now(),
            next_frame_at: None,
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
                let d = dialog::SkiaDialog::new(event_loop, data, &self.theme, false, sender, None);
                self.window_to_id.insert(d.window.id(), id);
                self.dialogs.insert(id, d);
                let _ = creation.send(Ok(receiver));
            }
            DialogMessageRequest::ShowProgressWindow(id, data, creation, on_button) => {
                let (sender, receiver) = oneshot::channel();
                let d = dialog::SkiaDialog::new(event_loop, data, &self.theme, true, sender, on_button);
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
        // Clamp the per-tick delta. The loop sleeps on ControlFlow::Wait between animations, so the
        // first tick after an idle gap reports the whole gap as elapsed. Without this clamp that
        // single huge step would jump an animation straight past short transitions (a 0.3s value
        // animation would advance ~2s in one frame and be discarded before it ever rendered).
        let elapsed_secs = elapsed.as_secs_f32().min(0.1);
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
        // Uncapped (max-throughput) benchmark phase: advance and request a redraw every iteration
        // and never sleep, so the loop renders as fast as it can.
        #[cfg(feature = "skia-instrumentation")]
        if instrument::uncapped() {
            self.tick();
            let mut any = false;
            for dialog in self.dialogs.values() {
                if dialog.needs_redraw() {
                    dialog.window.request_redraw();
                }
                any |= dialog.needs_redraw() || dialog.is_animating();
            }
            event_loop.set_control_flow(if any { ControlFlow::Poll } else { ControlFlow::Wait });
            return;
        }

        const FRAME_TIME: Duration = Duration::from_millis(16);
        let now = Instant::now();

        // Drive animations on a fixed ~60fps cadence. The next deadline is anchored to the previous
        // one rather than recomputed as `now + FRAME_TIME` on every wake-up: extra wake-ups between
        // frames (display-link redraw requests, input events) would otherwise keep sliding the
        // deadline forward, bunching renders into ~4ms bursts separated by ~21ms stalls — high
        // average FPS but visibly choppy. Anchoring keeps the spacing even.
        #[cfg(feature = "skia-instrumentation")]
        let mut due_tick = false;
        if self.dialogs.values().any(|d| d.is_animating()) {
            let due = self.next_frame_at.is_none_or(|t| now >= t);
            if due {
                #[cfg(feature = "skia-instrumentation")]
                {
                    due_tick = true;
                }
                // Starting a fresh run after an idle/non-animating gap: reset the tick clock so the
                // first frame advances ~0 instead of by the whole stale gap. `tick()` advances
                // animations by wall-clock elapsed (clamped to 0.1s); without this reset that first
                // step would jump a short hover animation straight to its end (snap, no animation).
                if self.next_frame_at.is_none() {
                    self.current_time = now;
                }
                self.tick();
                let next = self.next_frame_at.unwrap_or(now) + FRAME_TIME;
                // If we fell more than a frame behind, resync to `now` to avoid a catch-up burst.
                self.next_frame_at = Some(if next <= now { now + FRAME_TIME } else { next });
            }
        } else {
            self.next_frame_at = None;
        }

        // Request redraws for any dialog with pending content (this frame's tick, or an async
        // update such as a text change). The flag clears once the dialog renders.
        let mut pending = false;
        for dialog in self.dialogs.values() {
            if dialog.needs_redraw() {
                dialog.window.request_redraw();
                pending = true;
            }
        }

        // Record one pacing sample per scheduled animation frame, painted or not. Parked frames
        // (e.g. the indeterminate spinner's end-pauses) advance the cadence without repainting;
        // counting only painted frames would mis-read those intentional idle windows as stalls.
        #[cfg(feature = "skia-instrumentation")]
        if due_tick {
            instrument::record_tick(pending);
        }

        match self.next_frame_at {
            // Animating: wake at the next scheduled frame.
            Some(next) => event_loop.set_control_flow(ControlFlow::WaitUntil(next)),
            // A one-off redraw is queued; re-check shortly, then fall through to sleep.
            None if pending => event_loop.set_control_flow(ControlFlow::WaitUntil(now + FRAME_TIME)),
            // Idle: sleep until the next event.
            None => event_loop.set_control_flow(ControlFlow::Wait),
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
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                match state {
                    ElementState::Pressed => {
                        dialog.handle_mouse_pressed();
                    }
                    ElementState::Released => {
                        if let Some(index) = dialog.handle_mouse_released() {
                            let keep_open = dialog.on_button_clicked(dialog_id, index);
                            if !keep_open {
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
            WindowEvent::KeyboardInput { event, is_synthetic, .. }
                if event.state == ElementState::Pressed && !is_synthetic =>
            {
                match dialog.handle_key_pressed(&event.logical_key) {
                    KeyAction::ActivateButton(index) => {
                        if !event.repeat {
                            let keep_open = dialog.on_button_clicked(dialog_id, index);
                            if !keep_open {
                                dialog.window.set_visible(false);
                                let wid = window_id;
                                self.dialogs.remove(&dialog_id);
                                self.window_to_id.remove(&wid);
                            }
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
                DialogMessageRequest::ShowProgressWindow(_id, _options, creation, _on_button) => {
                    let _ = creation.send(Err(crate::XDialogError::NoBackendAvailable));
                }
                _ => {}
            }
        }
    }
}

impl XDialogBackendImpl for SkiaBackend {
    fn run_loop(receiver: Receiver<DialogMessageRequest>, xdialog_theme: XDialogTheme) {
        let mut builder = EventLoopBuilder::<DialogMessageRequest>::default();
        // On Linux the event loop may be built off the main thread (X11/Wayland);
        // macOS and Windows require it on the main thread, which the builder guarantees.
        #[cfg(target_os = "linux")]
        {
            use winit::platform::wayland::EventLoopBuilderExtWayland;
            use winit::platform::x11::EventLoopBuilderExtX11;
            EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
            EventLoopBuilderExtWayland::with_any_thread(&mut builder, true);
        }

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

        // Resolve the desktop appearance (light/dark + accent) once at startup; any failure
        // falls back to the hard-coded Ubuntu light theme.
        let appearance = desktop::resolve_appearance(xdialog_theme);
        let mut state = AppState::new(theme::get_theme(&appearance));
        if let Err(e) = event_loop.run_app(&mut state) {
            error!("xdialog: skia event loop error: {:?}", e);
        }

        // `run_app` returns once `main()` finishes and sends `ExitEventLoop`, so this fires
        // deterministically at the end of the benchmark, on the loop thread that recorded frames.
        #[cfg(feature = "skia-instrumentation")]
        instrument::report();
    }
}
