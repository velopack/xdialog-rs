//! Integrate xdialog into an event loop your application already runs.
//!
//! Works with winit of any version that speaks raw-window-handle 0.6 (winit 0.29 with the
//! `rwh_06` feature, 0.30, 0.31), or with any other windowing library that can hand out
//! raw-window-handle 0.6 handles. xdialog neither depends on nor exposes winit in this mode: the
//! only types that cross the boundary are this module's own ([`HostEvent`], [`WindowRequest`],
//! [`HostWindow`], ...) and `raw-window-handle` 0.6, which is a public dependency of this module.
//! You create and destroy the windows xdialog asks for and forward their events; xdialog renders
//! into them in software (softbuffer) with its Linux (egui) look.
//!
//! # Cargo setup
//!
//! Turn xdialog's default features off, so its own winit 0.30 event loop is not compiled at all
//! (your graph then contains only your winit):
//!
//! ```toml
//! [dependencies]
//! xdialog = { version = "4", default-features = false, features = ["winit-host"] }
//! ```
//!
//! On Windows this mode shows the Linux look; `init_win32_direct` (feature `win32-direct`) gives
//! native dialogs there. On macOS this module is a stub with the same API: [`init_winit_host`]
//! returns [`XDialogError::NoBackendAvailable`], installs nothing (so `XDialogBuilder` and
//! `maccf-direct` keep working), [`pump`] returns `None` and the other functions do nothing. The
//! glue below therefore compiles unchanged on all three platforms.
//!
//! # Contract
//!
//! Every function in this module must be called on the thread that runs your event loop, the
//! *host thread*. It is bound by the first [`pump`] (not by [`init_winit_host`], so you may
//! initialise on one thread and run the loop on another).
//!
//! 1. Call [`init_winit_host`] once, before any dialog function, passing a `waker` that makes your
//!    loop iterate (winit 0.29/0.30: `EventLoopProxy::send_event`; 0.31: `EventLoopProxy::wake_up`).
//!    The waker is called from arbitrary threads, possibly redundantly; it must not block, and it
//!    must not be lossy (a wake requested while the loop is busy must still produce an iteration).
//!    xdialog coalesces calls: at most one is outstanding between two [`pump`]s.
//! 2. Call [`pump`] after every wake and in every loop iteration (winit: `about_to_wait` /
//!    `Event::AboutToWait`), and wait no later than the deadline it returns
//!    (`ControlFlow::WaitUntil`), or indefinitely (`ControlFlow::Wait`) when it returns `None`.
//!    Dialog windows are created and destroyed inside `pump`.
//! 3. For every window created through [`HostWindows::create_window`], forward `RedrawRequested`
//!    to [`redraw`] and every other relevant event to [`handle_event`] (see [`HostEvent`]): resize,
//!    scale factor, pointer, mouse wheel, touch, modifiers, **non-synthetic** keys, focus, theme and
//!    close requests. Do not act on them yourself; in particular do not close the window on
//!    `CloseRequested`: xdialog answers the dialog and calls [`HostWindows::destroy_window`].
//! 4. Call [`shutdown`] before your windows, display connection or event loop are dropped (winit
//!    0.29: `Event::LoopExiting`; 0.30: `ApplicationHandler::exiting`; 0.31: right before
//!    `ActiveEventLoop::exit()`, since 0.31 has no `exiting` callback). This is a safety
//!    requirement, see [`HostWindows`].
//! 5. Dialog functions may be called from any thread. Worker threads simply block in
//!    `show_message*` until the user answers. On the host thread, `show_message*` (blocking)
//!    returns [`XDialogError::BlockingCallOnUiThread`] instead of deadlocking, and `show_progress*`
//!    returns its proxy immediately; the window appears on the next [`pump`] (a creation error is
//!    then only logged). The guard cannot detect every indirect block: a host thread that joins or
//!    waits on a worker which is itself inside `show_message` still deadlocks. Don't. Likewise,
//!    before the first [`pump`] xdialog cannot know which thread is the host thread: don't call
//!    dialog functions on it before your loop runs.
//! 6. Recommended: make xdialog's windows owned by / transient for your main window (winit on
//!    Windows: `WindowBuilderExtWindows::with_owner_window` / `WindowAttributesExtWindows::with_owner_window`),
//!    so they stay above it. `WindowRequest` carries no owner because xdialog cannot know your
//!    windows.
//!
//! Robustness: re-entrant calls (a toolkit that delivers events synchronously from inside
//! `create_window` / `set_visible`, or `pump` called from a progress callback) are queued and run
//! before the outer call returns; panics inside xdialog (including in your progress callbacks)
//! never unwind into your loop: the affected dialog is closed and the others keep running.
//! Calls from another thread than the host thread are a bug: they `debug_assert!` and are
//! otherwise ignored (a wrong-thread [`pump`] fails the queued dialog creations so their callers
//! don't hang).
//!
//! # Glue for winit 0.29
//!
//! The complete, runnable host is `examples/winit_host` in the xdialog repository (a separate
//! package on winit 0.29 whose `cargo tree` contains no winit 0.30):
//! `cargo run --manifest-path examples/winit_host/Cargo.toml` (add `-- --selftest` for a scripted
//! run with injected input). Its `src/lib.rs` is the glue: a window table, a [`HostWindows`] impl
//! over `&EventLoopWindowTarget` and a `WindowEvent` translation. The loop:
//!
//! ```ignore
//! event_loop.run(move |event, elwt| match event {
//!     Event::WindowEvent { window_id, event } => {
//!         if let Some(key) = xd.by_id.get(&window_id).copied() {
//!             let mut glue = Glue { elwt, windows: &mut xd };
//!             match event {
//!                 WindowEvent::RedrawRequested => host::redraw(&mut glue, key),
//!                 other => if let Some(ev) = translate(&other) { host::handle_event(&mut glue, key, ev) },
//!             }
//!         }
//!         // ... your own windows
//!     }
//!     Event::AboutToWait => {
//!         let next = host::pump(&mut Glue { elwt, windows: &mut xd });
//!         elwt.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
//!     }
//!     Event::LoopExiting => host::shutdown(&mut Glue { elwt, windows: &mut xd }),
//!     _ => {}
//! })?;
//! ```
//!
//! # Glue for winit 0.30
//!
//! ```ignore
//! use std::collections::HashMap;
//! use winit::application::ApplicationHandler;
//! use winit::dpi::{LogicalSize, PhysicalPosition};
//! use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
//! use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
//! use winit::keyboard::{Key as WKey, NamedKey};
//! use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
//! use winit::window::{Theme, Window, WindowButtons, WindowId};
//! use xdialog::host::{self, HostEvent, HostWindow, HostWindows, Key, MouseButton, ScrollDelta, WindowKey, WindowRequest};
//!
//! #[derive(Default)]
//! struct XdWindows {
//!     by_key: HashMap<WindowKey, Window>,
//!     by_id: HashMap<WindowId, WindowKey>,
//! }
//!
//! /// Borrowed for the duration of one `xdialog::host` call.
//! struct Glue<'a> {
//!     el: &'a ActiveEventLoop,
//!     windows: &'a mut XdWindows,
//! }
//!
//! // SAFETY: windows are dropped only in `destroy_window`; `host::shutdown` runs in `exiting`,
//! // before the windows and the event loop are dropped.
//! unsafe impl HostWindows for Glue<'_> {
//!     fn create_window(&mut self, req: &WindowRequest) -> Result<HostWindow, String> {
//!         let mut attrs = Window::default_attributes()
//!             .with_title(req.title.clone())
//!             .with_inner_size(LogicalSize::new(req.width, req.height))
//!             .with_resizable(req.resizable)
//!             .with_enabled_buttons(WindowButtons::CLOSE)
//!             .with_visible(req.visible)
//!             .with_active(req.active)
//!             .with_theme(if req.follow_system { None } else { Some(if req.dark { Theme::Dark } else { Theme::Light }) });
//!         if let Some((x, y)) = req.position {
//!             attrs = attrs.with_position(PhysicalPosition::new(x, y));
//!         } // else: centre it on the primary monitor (see the 0.29 example)
//!         let window = self.el.create_window(attrs).map_err(|e| e.to_string())?;
//!         let size = window.inner_size();
//!         let hw = HostWindow::new(
//!             window.display_handle().map_err(|e| e.to_string())?.as_raw(),
//!             window.window_handle().map_err(|e| e.to_string())?.as_raw(),
//!             window.scale_factor(),
//!             (size.width, size.height),
//!         );
//!         self.windows.by_id.insert(window.id(), req.key);
//!         self.windows.by_key.insert(req.key, window);
//!         Ok(hw)
//!     }
//!     fn set_visible(&mut self, key: WindowKey, visible: bool) {
//!         if let Some(w) = self.windows.by_key.get(&key) { w.set_visible(visible) }
//!     }
//!     fn set_inner_size(&mut self, key: WindowKey, width: f64, height: f64) {
//!         if let Some(w) = self.windows.by_key.get(&key) { let _ = w.request_inner_size(LogicalSize::new(width, height)); }
//!     }
//!     fn request_redraw(&mut self, key: WindowKey) {
//!         if let Some(w) = self.windows.by_key.get(&key) { w.request_redraw() }
//!     }
//!     fn set_dark(&mut self, key: WindowKey, dark: bool) {
//!         if let Some(w) = self.windows.by_key.get(&key) { w.set_theme(Some(if dark { Theme::Dark } else { Theme::Light })) }
//!     }
//!     fn destroy_window(&mut self, key: WindowKey) {
//!         if let Some(w) = self.windows.by_key.remove(&key) { self.windows.by_id.remove(&w.id()); }
//!     }
//! }
//!
//! /// winit 0.30 -> xdialog. `Touch` and `ModifiersChanged` map exactly as in the 0.29 example.
//! fn translate(event: &WindowEvent) -> Option<HostEvent> {
//!     Some(match event {
//!         WindowEvent::Resized(s) => HostEvent::Resized { width: s.width, height: s.height },
//!         WindowEvent::ScaleFactorChanged { scale_factor, .. } => HostEvent::ScaleFactorChanged { scale_factor: *scale_factor },
//!         WindowEvent::CursorMoved { position, .. } => HostEvent::CursorMoved { x: position.x, y: position.y },
//!         WindowEvent::CursorLeft { .. } => HostEvent::CursorLeft,
//!         WindowEvent::MouseInput { state, button, .. } => HostEvent::MouseButton {
//!             button: match button {
//!                 winit::event::MouseButton::Left => MouseButton::Primary,
//!                 winit::event::MouseButton::Right => MouseButton::Secondary,
//!                 winit::event::MouseButton::Middle => MouseButton::Middle,
//!                 _ => MouseButton::Other,
//!             },
//!             pressed: *state == ElementState::Pressed,
//!         },
//!         WindowEvent::MouseWheel { delta, .. } => HostEvent::MouseWheel {
//!             delta: match *delta {
//!                 MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines { x, y },
//!                 MouseScrollDelta::PixelDelta(p) => ScrollDelta::Pixels { x: p.x, y: p.y },
//!             },
//!         },
//!         // Synthetic key events (sent on focus changes) must not be forwarded.
//!         WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
//!             let key = match &event.logical_key {
//!                 WKey::Named(NamedKey::Enter) => Key::Enter,
//!                 WKey::Named(NamedKey::Escape) => Key::Escape,
//!                 WKey::Named(NamedKey::Tab) => Key::Tab,
//!                 WKey::Named(NamedKey::Space) => Key::Space,
//!                 WKey::Named(NamedKey::ArrowLeft) => Key::ArrowLeft,
//!                 WKey::Named(NamedKey::ArrowRight) => Key::ArrowRight,
//!                 // ... ArrowUp/ArrowDown, Home/End, PageUp/PageDown likewise
//!                 _ => return None,
//!             };
//!             HostEvent::Key { key, pressed: event.state == ElementState::Pressed, repeat: event.repeat }
//!         }
//!         WindowEvent::Focused(f) => HostEvent::Focused(*f),
//!         WindowEvent::ThemeChanged(_) => HostEvent::ThemeChanged,
//!         WindowEvent::CloseRequested => HostEvent::CloseRequested,
//!         _ => return None,
//!     })
//! }
//!
//! enum UserEvent { XDialogWake }
//!
//! struct App { xd: XdWindows }
//!
//! impl ApplicationHandler<UserEvent> for App {
//!     fn resumed(&mut self, _el: &ActiveEventLoop) {}
//!
//!     // Waking is enough: `about_to_wait` follows and pumps.
//!     fn user_event(&mut self, _el: &ActiveEventLoop, _event: UserEvent) {}
//!
//!     fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
//!         if let Some(key) = self.xd.by_id.get(&id).copied() {
//!             let mut glue = Glue { el, windows: &mut self.xd };
//!             match event {
//!                 WindowEvent::RedrawRequested => host::redraw(&mut glue, key),
//!                 other => if let Some(ev) = translate(&other) { host::handle_event(&mut glue, key, ev) },
//!             }
//!             return;
//!         }
//!         // ... your own windows
//!     }
//!
//!     fn about_to_wait(&mut self, el: &ActiveEventLoop) {
//!         let next = host::pump(&mut Glue { el, windows: &mut self.xd });
//!         el.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
//!     }
//!
//!     // REQUIRED: release xdialog's surfaces and windows while the display is still open.
//!     fn exiting(&mut self, el: &ActiveEventLoop) {
//!         host::shutdown(&mut Glue { el, windows: &mut self.xd });
//!     }
//! }
//!
//! fn main() {
//!     let event_loop = EventLoop::<UserEvent>::with_user_event().build().unwrap();
//!     let proxy = event_loop.create_proxy();
//!     xdialog::init_winit_host(xdialog::XDialogTheme::SystemDefault, move || {
//!         let _ = proxy.send_event(UserEvent::XDialogWake);
//!     })
//!     .unwrap();
//!     std::thread::spawn(|| xdialog::show_message_info_ok("Hosted", "winit 0.30", "Shown from a worker thread."));
//!     event_loop.run_app(&mut App { xd: XdWindows::default() }).unwrap();
//! }
//! ```
//!
//! # Glue for winit 0.31 (beta)
//!
//! Checked against `winit 0.31.0-beta.3`. The differences from 0.30:
//! - Windows are `Box<dyn Window>`, created from `WindowAttributes::default()`; "inner size" is
//!   now "surface size" (`with_surface_size`, `surface_size`, `request_surface_size`,
//!   `WindowEvent::SurfaceResized`); handles come from `rwh_06_display_handle()` /
//!   `rwh_06_window_handle()`.
//! - The event loop has no user-event type: wake with `EventLoopProxy::wake_up()`, which lands in
//!   `ApplicationHandler::proxy_wake_up`. (winit may merge wake-ups that are still pending; that is
//!   fine, because [`pump`] re-reads its queue after clearing its wake flag.)
//! - Mouse and touch are unified pointer events: forward those with `primary: true`
//!   (`PointerMoved` / `PointerLeft` / `PointerButton`); there is no separate touch mapping.
//! - Space is `Key::Character(" ")`, not a `NamedKey`.
//! - There is no `exiting` callback: call [`shutdown`] yourself right before
//!   `ActiveEventLoop::exit()`.
//!
//! ```ignore
//! use std::collections::HashMap;
//! use winit::application::ApplicationHandler;
//! use winit::dpi::{LogicalSize, PhysicalPosition};
//! use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
//! use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
//! use winit::keyboard::{Key as WKey, NamedKey};
//! use winit::window::{Theme, Window, WindowAttributes, WindowButtons, WindowId};
//! use xdialog::host::{self, HostEvent, HostWindow, HostWindows, Key, MouseButton, ScrollDelta, WindowKey, WindowRequest};
//!
//! #[derive(Default)]
//! struct XdWindows {
//!     by_key: HashMap<WindowKey, Box<dyn Window>>,
//!     by_id: HashMap<WindowId, WindowKey>,
//! }
//!
//! struct Glue<'a> {
//!     el: &'a dyn ActiveEventLoop,
//!     windows: &'a mut XdWindows,
//! }
//!
//! // SAFETY: windows are dropped only in `destroy_window`; `host::shutdown` runs before
//! // `exit()`, i.e. before the windows and the event loop are dropped.
//! unsafe impl HostWindows for Glue<'_> {
//!     fn create_window(&mut self, req: &WindowRequest) -> Result<HostWindow, String> {
//!         let mut attrs = WindowAttributes::default()
//!             .with_title(req.title.clone())
//!             .with_surface_size(LogicalSize::new(req.width, req.height))
//!             .with_resizable(req.resizable)
//!             .with_enabled_buttons(WindowButtons::CLOSE)
//!             .with_visible(req.visible)
//!             .with_active(req.active)
//!             .with_theme(if req.follow_system { None } else { Some(if req.dark { Theme::Dark } else { Theme::Light }) });
//!         if let Some((x, y)) = req.position {
//!             attrs = attrs.with_position(PhysicalPosition::new(x, y));
//!         }
//!         let window = self.el.create_window(attrs).map_err(|e| e.to_string())?;
//!         let size = window.surface_size();
//!         let hw = HostWindow::new(
//!             window.rwh_06_display_handle().display_handle().map_err(|e| e.to_string())?.as_raw(),
//!             window.rwh_06_window_handle().window_handle().map_err(|e| e.to_string())?.as_raw(),
//!             window.scale_factor(),
//!             (size.width, size.height),
//!         );
//!         self.windows.by_id.insert(window.id(), req.key);
//!         self.windows.by_key.insert(req.key, window);
//!         Ok(hw)
//!     }
//!     fn set_visible(&mut self, key: WindowKey, visible: bool) {
//!         if let Some(w) = self.windows.by_key.get(&key) { w.set_visible(visible) }
//!     }
//!     fn set_inner_size(&mut self, key: WindowKey, width: f64, height: f64) {
//!         if let Some(w) = self.windows.by_key.get(&key) { let _ = w.request_surface_size(LogicalSize::new(width, height).into()); }
//!     }
//!     fn request_redraw(&mut self, key: WindowKey) {
//!         if let Some(w) = self.windows.by_key.get(&key) { w.request_redraw() }
//!     }
//!     fn set_dark(&mut self, key: WindowKey, dark: bool) {
//!         if let Some(w) = self.windows.by_key.get(&key) { w.set_theme(Some(if dark { Theme::Dark } else { Theme::Light })) }
//!     }
//!     fn destroy_window(&mut self, key: WindowKey) {
//!         if let Some(w) = self.windows.by_key.remove(&key) { self.windows.by_id.remove(&w.id()); }
//!     }
//! }
//!
//! fn translate(event: &WindowEvent) -> Option<HostEvent> {
//!     Some(match event {
//!         WindowEvent::SurfaceResized(s) => HostEvent::Resized { width: s.width, height: s.height },
//!         WindowEvent::ScaleFactorChanged { scale_factor, .. } => HostEvent::ScaleFactorChanged { scale_factor: *scale_factor },
//!         WindowEvent::PointerMoved { position, primary: true, .. } => HostEvent::CursorMoved { x: position.x, y: position.y },
//!         WindowEvent::PointerLeft { primary: true, .. } => HostEvent::CursorLeft,
//!         WindowEvent::PointerButton { state, primary: true, button, .. } => HostEvent::MouseButton {
//!             // A primary touch reports `MouseButton::Left`.
//!             button: match button.clone().mouse_button() {
//!                 Some(winit::event::MouseButton::Left) => MouseButton::Primary,
//!                 Some(winit::event::MouseButton::Right) => MouseButton::Secondary,
//!                 Some(winit::event::MouseButton::Middle) => MouseButton::Middle,
//!                 _ => MouseButton::Other,
//!             },
//!             pressed: *state == ElementState::Pressed,
//!         },
//!         WindowEvent::MouseWheel { delta, .. } => HostEvent::MouseWheel {
//!             delta: match *delta {
//!                 MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines { x, y },
//!                 MouseScrollDelta::PixelDelta(p) => ScrollDelta::Pixels { x: p.x, y: p.y },
//!                 _ => return None,
//!             },
//!         },
//!         WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
//!             let key = match &event.logical_key {
//!                 WKey::Named(NamedKey::Enter) => Key::Enter,
//!                 WKey::Named(NamedKey::Escape) => Key::Escape,
//!                 WKey::Named(NamedKey::Tab) => Key::Tab,
//!                 WKey::Character(c) if c.as_str() == " " => Key::Space,
//!                 // ... arrows, Home/End, PageUp/PageDown as for 0.30
//!                 _ => return None,
//!             };
//!             HostEvent::Key { key, pressed: event.state == ElementState::Pressed, repeat: event.repeat }
//!         }
//!         WindowEvent::Focused(f) => HostEvent::Focused(*f),
//!         WindowEvent::ThemeChanged(_) => HostEvent::ThemeChanged,
//!         WindowEvent::CloseRequested => HostEvent::CloseRequested,
//!         _ => return None,
//!     })
//! }
//!
//! struct App { xd: XdWindows }
//!
//! impl App {
//!     /// Quit: release xdialog's windows first (REQUIRED; 0.31 has no `exiting` callback).
//!     fn quit(&mut self, el: &dyn ActiveEventLoop) {
//!         host::shutdown(&mut Glue { el, windows: &mut self.xd });
//!         el.exit();
//!     }
//! }
//!
//! impl ApplicationHandler for App {
//!     fn can_create_surfaces(&mut self, _el: &dyn ActiveEventLoop) {}
//!
//!     // `EventLoopProxy::wake_up` lands here; `about_to_wait` follows and pumps.
//!     fn proxy_wake_up(&mut self, _el: &dyn ActiveEventLoop) {}
//!
//!     fn window_event(&mut self, el: &dyn ActiveEventLoop, id: WindowId, event: WindowEvent) {
//!         if let Some(key) = self.xd.by_id.get(&id).copied() {
//!             let mut glue = Glue { el, windows: &mut self.xd };
//!             match event {
//!                 WindowEvent::RedrawRequested => host::redraw(&mut glue, key),
//!                 other => if let Some(ev) = translate(&other) { host::handle_event(&mut glue, key, ev) },
//!             }
//!             return;
//!         }
//!         if let WindowEvent::CloseRequested = event {
//!             self.quit(el); // your main window
//!         }
//!     }
//!
//!     fn about_to_wait(&mut self, el: &dyn ActiveEventLoop) {
//!         let next = host::pump(&mut Glue { el, windows: &mut self.xd });
//!         el.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
//!     }
//! }
//!
//! fn main() {
//!     let event_loop = EventLoop::new().unwrap();
//!     let proxy = event_loop.create_proxy();
//!     xdialog::init_winit_host(xdialog::XDialogTheme::SystemDefault, move || proxy.wake_up()).unwrap();
//!     event_loop.run_app(App { xd: XdWindows::default() }).unwrap();
//! }
//! ```
//!
//! # Without winit
//!
//! Nothing here requires winit. Any toolkit works if it can create a top-level window, return its
//! raw-window-handle 0.6 display and window handles (Win32, Xlib, Xcb or Wayland), resize, show
//! and hide it, deliver a redraw callback, and wake its loop from another thread. Map its events
//! to [`HostEvent`] (physical pixels, client-relative) and follow the contract above.
//!
//! # Limitations
//!
//! - The title-bar colour follows [`WindowRequest::dark`] at creation and later
//!   [`HostWindows::set_dark`] calls. `set_dark` is optional: without it your window's decorations
//!   keep their creation colour when the appearance changes (the dialog content still follows
//!   [`HostEvent::ThemeChanged`]).
//! - Before the first dialog window exists, xdialog does not know the monitor: it lays out the
//!   first dialog at scale 1 and re-requests its size ([`HostWindows::set_inner_size`]) when the
//!   window reports a different scale, and caps the height at 800 logical px unless
//!   [`HostWindow::with_work_area_height`] says otherwise.
//! - After [`shutdown`], host mode cannot be started again in the same process; every dialog
//!   function returns [`XDialogError::NoBackendAvailable`].

use std::time::Instant;

use crate::{XDialogError, XDialogTheme};

pub use crate::backends::host_types::{
    HostEvent, HostWindow, HostWindows, Key, Modifiers, MouseButton, ScrollDelta, TouchPhase, WindowKey, WindowRequest,
};

/// Initialize xdialog in host-integration mode (see [the module docs](self)).
///
/// Call once, before any dialog function. May be called on any thread; the host thread is bound by
/// the first [`pump`], so requests made before the loop runs simply wait in a queue. `theme`
/// selects light, dark or the system appearance for every dialog.
///
/// `waker` is invoked from any thread when xdialog needs the host to call [`pump`]: after a dialog
/// request, when a background thread found new fonts or an appearance change, and after
/// [`handle_event`] / [`redraw`] scheduled a frame sooner than the deadline [`pump`] last returned.
/// It must make the loop iterate (winit: `EventLoopProxy::send_event` / `wake_up`), must not block
/// and must not be lossy. Calls are coalesced (at most one outstanding per [`pump`]); a panic in
/// the waker is caught and logged.
///
/// # Errors
/// - [`XDialogError::SystemError`] if a request handler was already installed (another `init_*`
///   or an `XDialogBuilder` ran first); the waker will then never be called.
/// - [`XDialogError::NoBackendAvailable`] on macOS (stub).
pub fn init_winit_host(theme: XDialogTheme, waker: impl Fn() + Send + Sync + 'static) -> Result<(), XDialogError> {
    imp::init_winit_host(theme, Box::new(waker))
}

/// Process queued dialog requests (creating and destroying windows through `host` as needed) and
/// fire due timers ([`HostWindows::request_redraw`] for dialogs that need a frame).
///
/// Returns the earliest time xdialog needs [`pump`] to run again: `None` when idle (wait for the
/// next event or wake), `Some(t)` while something animates (about every 16 ms) or when more work
/// arrived during this call (`t` = now). Call it in every loop iteration (winit: `about_to_wait`)
/// and after every wake.
///
/// The first call binds the host thread and marks it as an xdialog UI thread (blocking dialog
/// calls on it return [`XDialogError::BlockingCallOnUiThread`]). Before [`init_winit_host`] and
/// after [`shutdown`] it does nothing and returns `None`.
pub fn pump(host: &mut dyn HostWindows) -> Option<Instant> {
    imp::pump(host)
}

/// Forward an input or window event for one of xdialog's windows (`key` from
/// [`WindowRequest::key`]). Events for unknown or already-destroyed windows are ignored.
///
/// Input is applied immediately; the visual response comes with the next [`redraw`]. When the
/// event needs a frame, xdialog calls [`HostWindows::request_redraw`] right away or, for a later
/// frame, the waker, so a host that is already waiting still wakes up in time. Answering a dialog
/// (button, Enter, Escape, `CloseRequested`) destroys its window through
/// [`HostWindows::destroy_window`] before this call returns.
pub fn handle_event(host: &mut dyn HostWindows, key: WindowKey, event: HostEvent) {
    imp::handle_event(host, key, event)
}

/// Forward `RedrawRequested` for one of xdialog's windows: xdialog renders and presents a frame
/// into it (software, through softbuffer). Unknown keys are ignored.
pub fn redraw(host: &mut dyn HostWindows, key: WindowKey) {
    imp::redraw(host, key)
}

/// Close every open dialog (their callers receive `WindowClosed`, i.e. `Ok(false)` from the
/// yes/no style functions), release every surface, call [`HostWindows::destroy_window`] for each
/// window, and drop xdialog's host state. Afterwards every dialog function returns
/// [`XDialogError::NoBackendAvailable`] (queued requests too), and host mode cannot be restarted in
/// this process. Idempotent.
///
/// Required before your windows, display connection or event loop are dropped (see
/// [`HostWindows`]). If the host thread exits without it, xdialog leaks its surfaces rather than
/// release them against a display that may already be closed, and logs an error.
pub fn shutdown(host: &mut dyn HostWindows) {
    imp::shutdown(host)
}

/// The real implementation (Linux, Windows): `egui_core::host`.
#[cfg(xd_winit_host)]
use crate::backends::egui_core::host as imp;

/// macOS stub: installs nothing, so AppKit via `XDialogBuilder` (or `maccf-direct`) keeps working.
#[cfg(xd_winit_host_stub)]
mod imp {
    use super::*;

    pub(super) fn init_winit_host(_theme: XDialogTheme, _waker: Box<dyn Fn() + Send + Sync + 'static>) -> Result<(), XDialogError> {
        Err(XDialogError::NoBackendAvailable)
    }
    pub(super) fn pump(_host: &mut dyn HostWindows) -> Option<Instant> {
        None
    }
    pub(super) fn handle_event(_host: &mut dyn HostWindows, _key: WindowKey, _event: HostEvent) {}
    pub(super) fn redraw(_host: &mut dyn HostWindows, _key: WindowKey) {}
    pub(super) fn shutdown(_host: &mut dyn HostWindows) {}
}
