//! xdialog's own winit 0.30 event loop.
//!
//! - [`build_event_loop`]: `with_any_thread(true)` on every platform (tests and `XDialogBuilder`
//!   run on arbitrary threads; linux-direct on its own thread), Windows `with_dpi_aware(false)`
//!   (the loop thread sets per-monitor-v2 awareness itself; a library must not change process DPI
//!   state). A panic inside `build()` is caught.
//! - [`OwnLoopApp`]: the `ApplicationHandler`, generic over the theme. It owns a [`Manager`] with
//!   the winit window system ([`WinitWs`], created per callback) and translates winit window
//!   events into the version-neutral `HostEvent`s every mode shares.
//! - [`run_builder`]: builder mode on the caller's thread (the user's main thread): requests are
//!   forwarded from the channel into the loop by a small thread; `ExitEventLoop` closes every
//!   dialog and exits; the thread's DPI awareness and UI-thread mark are restored afterwards.
//! - linux-direct (`direct.rs`) builds the loop with [`build_event_loop`] on its own thread
//!   and runs [`OwnLoopApp`] with an [`Inbox`] (drained on [`UserEvent::Wake`]) and
//!   `exit_on_request = false`.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::Vec2;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy, OwnedDisplayHandle};
use winit::window::{Window, WindowButtons, WindowId};

use super::manager::{request_id, Manager};
use super::render::{Presenter, SoftwarePresenter};
use super::theme::Theme;
use super::window_system::{CreatedWindow, WindowSpec, WindowSystem};
use crate::backends::host_types::{HostEvent, Key, Modifiers, MouseButton, ScrollDelta, TouchPhase};
use crate::model::{DialogMessageRequest, XDialogTheme};
use crate::XDialogError;

/// Events sent to the loop through its `EventLoopProxy`.
#[derive(Debug)]
pub(crate) enum UserEvent {
    /// A dialog request (builder mode: forwarded from the request channel).
    Request(DialogMessageRequest),
    /// Drain the [`Inbox`] (linux-direct).
    Wake,
    /// Fonts or appearance changed on a background thread: refresh open dialogs.
    Refresh,
    /// A test-hook command for a live dialog.
    #[cfg(xd_test_hooks)]
    Remote(super::manager::live::RemoteCmd),
}

/// Requests delivered through a channel plus a wake flag (linux-direct): the sender
/// pushes to `rx`'s channel, then sends [`UserEvent::Wake`] if `wake_pending` was false. The loop
/// clears `wake_pending` BEFORE draining, so no request is ever stranded.
pub(crate) struct Inbox {
    pub rx: Receiver<DialogMessageRequest>,
    pub wake_pending: Arc<AtomicBool>,
}

/// Why [`build_event_loop`] failed.
#[derive(Debug)]
pub(crate) enum LoopBuildError {
    /// This process already created a winit 0.30 event loop (winit allows one per process).
    Recreation,
    /// No display server, or another error / panic inside `build()`.
    Other(String),
}

impl std::fmt::Display for LoopBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoopBuildError::Recreation => f.write_str("a winit 0.30 event loop was already created in this process"),
            LoopBuildError::Other(s) => f.write_str(s),
        }
    }
}

/// Build xdialog's winit event loop (see the module docs). Never panics.
pub(crate) fn build_event_loop() -> Result<EventLoop<UserEvent>, LoopBuildError> {
    let built = catch_unwind(AssertUnwindSafe(|| {
                                 let mut b = EventLoop::<UserEvent>::with_user_event();
                                 #[cfg(windows)]
                                 {
                                     use winit::platform::windows::EventLoopBuilderExtWindows;
                                     b.with_any_thread(true);
                                     b.with_dpi_aware(false);
                                 }
                                 #[cfg(target_os = "linux")]
                                 {
                                     use winit::platform::wayland::EventLoopBuilderExtWayland;
                                     use winit::platform::x11::EventLoopBuilderExtX11;
                                     EventLoopBuilderExtX11::with_any_thread(&mut b, true);
                                     EventLoopBuilderExtWayland::with_any_thread(&mut b, true);
                                 }
                                 b.build()
                             }));
    match built {
        Ok(Ok(el)) => Ok(el),
        Ok(Err(winit::error::EventLoopError::RecreationAttempt)) => Err(LoopBuildError::Recreation),
        Ok(Err(e)) => Err(LoopBuildError::Other(format!("could not create the event loop (no display server?): {e}"))),
        Err(_) => Err(LoopBuildError::Other("building the event loop panicked".into())),
    }
}

/// The own loop could not be built (an `Err` or a panic inside `EventLoop::build`). Nothing has
/// consumed `receiver`, so the caller can fall back (Win32 on Windows, `NoBackendAvailable`
/// elsewhere).
pub(crate) struct BuildFailed {
    pub receiver: Receiver<DialogMessageRequest>,
    pub reason: String,
}

/// Run the builder-mode loop on the current thread until `ExitEventLoop`.
/// Returns `Err(BuildFailed)` without touching `receiver` if the event loop can't be built.
pub(crate) fn run_builder<T: Theme>(theme: T, receiver: Receiver<DialogMessageRequest>, xtheme: XDialogTheme) -> Result<(), BuildFailed> {
    // Per-monitor-v2 for this thread only, restored when the builder returns (user's main thread).
    #[cfg(windows)]
    let _dpi = super::platform_win::ThreadDpiGuard::per_monitor_v2();

    let event_loop = match build_event_loop() {
        Ok(el) => el,
        Err(e) => return Err(BuildFailed { receiver, reason: e.to_string() }),
    };
    let proxy = event_loop.create_proxy();

    // Forward requests into the loop. The receiver is parked in a slot so a failed spawn can hand
    // it back for the fallback backend.
    let slot = Arc::new(Mutex::new(Some(receiver)));
    let fwd_slot = slot.clone();
    let fwd = proxy.clone();
    let spawned = std::thread::Builder::new().name("xdialog-requests".into()).spawn(move || {
                                                                                 let rx = fwd_slot.lock().unwrap_or_else(|e| e.into_inner()).take();
                                                                                 let Some(rx) = rx else { return };
                                                                                 while let Ok(m) = rx.recv() {
                                                                                     if fwd.send_event(UserEvent::Request(m)).is_err() {
                                                                                         break;
                                                                                     }
                                                                                 }
                                                                             });
    if let Err(e) = spawned {
        let receiver = slot.lock().unwrap_or_else(|e| e.into_inner()).take().expect("receiver not taken");
        return Err(BuildFailed { receiver, reason: format!("could not start the request thread: {e}") });
    }

    let _ui_thread = UiThreadMark::set();
    let mut app = OwnLoopApp::new(theme, xtheme, proxy, None, true);
    if let Err(e) = event_loop.run_app(&mut app) {
        error!("xdialog: event loop error: {e}");
    }
    drop(app);
    Ok(())
}

/// Marks the current thread as an xdialog UI thread (blocking-call guard) and unmarks
/// it when dropped, also when the loop unwinds (builder mode runs on the user's main thread).
pub(crate) struct UiThreadMark;

impl UiThreadMark {
    pub(crate) fn set() -> Self {
        crate::channel::mark_ui_thread(true);
        UiThreadMark
    }
}

impl Drop for UiThreadMark {
    fn drop(&mut self) {
        crate::channel::mark_ui_thread(false);
    }
}

// -------------------------------------------------------------------------------------------------
// Window system
// -------------------------------------------------------------------------------------------------

/// A dialog window of the own loop.
pub(crate) struct WinitWin {
    pub window: Rc<Window>,
    pub id: WindowId,
    /// Cached monitor refresh period and when it was read (re-read about once a second, so a
    /// window dragged to another monitor picks up its rate).
    period: std::cell::Cell<Option<(Instant, Option<Duration>)>>,
}

/// The winit window system, created fresh inside each `ApplicationHandler` callback.
pub(crate) struct WinitWs<'a> {
    pub el: &'a ActiveEventLoop,
    pub sb: Option<&'a softbuffer::Context<OwnedDisplayHandle>>,
}

impl WinitWs<'_> {
    fn primary(&self) -> Option<winit::monitor::MonitorHandle> {
        self.el.primary_monitor().or_else(|| self.el.available_monitors().next())
    }
}

/// The theme a new window is created with. On Windows, winit only reports `ThemeChanged` (on
/// `WM_SETTINGCHANGE`) for windows created WITHOUT a preferred theme, so a dialog that follows the
/// system gets `None` there (winit then applies the system theme, which is the dialog's too); the
/// title bar is set through DWM right after creation and on every appearance change.
fn window_theme(spec: &WindowSpec<'_>) -> Option<winit::window::Theme> {
    if cfg!(windows) && spec.follow_system {
        None
    } else {
        Some(winit_theme(spec.dark_titlebar))
    }
}

fn winit_theme(dark: bool) -> winit::window::Theme {
    if dark {
        winit::window::Theme::Dark
    } else {
        winit::window::Theme::Light
    }
}

impl WindowSystem for WinitWs<'_> {
    type Win = WinitWin;

    fn monitor_period(&self, w: &WinitWin) -> Option<Duration> {
        let now = Instant::now();
        if let Some((at, period)) = w.period.get() {
            if now.duration_since(at) < Duration::from_secs(1) {
                return period;
            }
        }
        let period = w.window.current_monitor()
                      .and_then(|m| m.refresh_rate_millihertz())
                      .filter(|&mhz| mhz > 0)
                      .map(|mhz| Duration::from_secs_f64(1000.0 / mhz as f64));
        w.period.set(Some((now, period)));
        period
    }

    fn create(&mut self, spec: &WindowSpec<'_>) -> Result<CreatedWindow<WinitWin>, XDialogError> {
        let sb = self.sb.ok_or_else(|| XDialogError::SystemError("xdialog: the software presenter is unavailable".into()))?;
        let mut attrs = Window::default_attributes().with_title(spec.title)
                                                    .with_inner_size(LogicalSize::new(spec.inner_size.x as f64, spec.inner_size.y as f64))
                                                    .with_resizable(false)
                                                    .with_enabled_buttons(WindowButtons::CLOSE)
                                                    .with_visible(false)
                                                    .with_active(spec.active)
                                                    .with_theme(window_theme(spec));
        let position = spec.position.or_else(|| {
                                        // Centred on the primary monitor (physical), computed up front so the WM
                                        // doesn't reposition after mapping (skia behaviour).
                                        let m = self.primary()?;
                                        let (size, pos, scale) = (m.size(), m.position(), m.scale_factor());
                                        let w = (spec.inner_size.x as f64 * scale).round() as i32;
                                        let h = (spec.inner_size.y as f64 * scale).round() as i32;
                                        Some([pos.x + (size.width as i32 - w) / 2, pos.y + (size.height as i32 - h) / 2])
                                    });
        if let Some([x, y]) = position {
            attrs = attrs.with_position(PhysicalPosition::new(x, y));
        }
        let window = self.el.create_window(attrs).map_err(|e| XDialogError::SystemError(format!("xdialog: could not create a window: {e}")))?;
        let window = Rc::new(window);
        #[cfg(windows)]
        super::platform_win::apply_dwm_attributes(raw_id(&window), spec.dark_titlebar);
        let presenter = SoftwarePresenter::new(sb, window.clone()).map_err(|e| XDialogError::SystemError(format!("xdialog: could not create a surface: {e}")))?;
        let ppp = window.scale_factor() as f32;
        let size = window.inner_size();
        let id = window.id();
        Ok(CreatedWindow { win: WinitWin { window, id, period: Default::default() }, presenter: Box::new(presenter), ppp, size_px: [size.width, size.height] })
    }

    fn set_visible(&mut self, w: &WinitWin, visible: bool) {
        w.window.set_visible(visible);
    }

    fn request_inner_size(&mut self, w: &WinitWin, logical: Vec2) -> Option<[u32; 2]> {
        // `Some`: applied at once and winit may not emit `Resized` for it (Wayland).
        w.window.request_inner_size(LogicalSize::new(logical.x as f64, logical.y as f64)).map(|s| [s.width, s.height])
    }

    fn request_redraw(&mut self, w: &WinitWin) {
        w.window.request_redraw();
    }

    fn destroy(&mut self, w: WinitWin) {
        // Dropping the last Rc destroys the window (the presenter's clone was dropped first).
        drop(w);
    }

    fn max_client_height(&self) -> Option<f32> {
        let m = self.primary()?;
        Some((m.size().height as f64 / m.scale_factor() * 0.9) as f32)
    }

    fn expected_ppp(&self) -> f32 {
        self.primary().map_or(1.0, |m| m.scale_factor() as f32)
    }

    fn raw_window_id(&self, w: &WinitWin) -> isize {
        raw_id(&w.window)
    }

    fn recreate_presenter(&mut self, w: &WinitWin) -> Option<Box<dyn Presenter>> {
        let sb = self.sb?;
        match SoftwarePresenter::new(sb, w.window.clone()) {
            Ok(p) => Some(Box::new(p)),
            Err(e) => {
                warn!("xdialog: could not recreate the surface: {e}");
                None
            }
        }
    }

    fn virtual_screen_left(&self) -> i32 {
        #[cfg(windows)]
        {
            super::platform_win::virtual_screen_left()
        }
        #[cfg(not(windows))]
        {
            self.el.available_monitors().map(|m| m.position().x).min().unwrap_or(0)
        }
    }

    fn set_dark_titlebar(&mut self, w: &WinitWin, dark: bool) {
        // Windows: DWM directly (`Window::set_theme` doesn't change winit's preferred theme, and
        // `ThemeChanged` must keep flowing for system-following dialogs).
        #[cfg(windows)]
        super::platform_win::apply_dwm_attributes(raw_id(&w.window), dark);
        #[cfg(not(windows))]
        w.window.set_theme(Some(winit_theme(dark)));
    }
}

/// HWND (Windows) / XID (X11) of a window, 0 otherwise.
fn raw_id(window: &Window) -> isize {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
        Ok(RawWindowHandle::Xlib(h)) => h.window as isize,
        Ok(RawWindowHandle::Xcb(h)) => h.window.get() as isize,
        _ => 0,
    }
}

// -------------------------------------------------------------------------------------------------
// Application handler
// -------------------------------------------------------------------------------------------------

/// The own loop's `ApplicationHandler` (builder and linux-direct).
pub(crate) struct OwnLoopApp<T: Theme> {
    manager: Manager<T, WinitWin>,
    /// One softbuffer context per loop (created in `resumed`).
    sb: Option<softbuffer::Context<OwnedDisplayHandle>>,
    /// `resumed` has run (windows may be created).
    ready: bool,
    /// Requests that arrived before `resumed`.
    queued: Vec<DialogMessageRequest>,
    inbox: Option<Inbox>,
    /// Builder mode: `ExitEventLoop` exits the loop. linux-direct: the loop never exits.
    exit_on_request: bool,
}

impl<T: Theme> OwnLoopApp<T> {
    /// `proxy` wakes the loop from background threads (fonts / appearance / test hooks).
    pub(crate) fn new(theme: T, xtheme: XDialogTheme, proxy: EventLoopProxy<UserEvent>, inbox: Option<Inbox>, exit_on_request: bool) -> Self {
        let refresh = proxy.clone();
        let waker: super::fonts::Waker = Arc::new(move || {
                                                    let _ = refresh.send_event(UserEvent::Refresh);
                                                });
        #[allow(unused_mut)]
        let mut manager = Manager::new(theme, xtheme, Some(waker));
        #[cfg(xd_test_hooks)]
        {
            let remote = proxy.clone();
            manager.set_remote(Arc::new(move |cmd| {
                                   let _ = remote.send_event(UserEvent::Remote(cmd));
                               }));
        }
        let _ = proxy;
        OwnLoopApp { manager, sb: None, ready: false, queued: Vec::new(), inbox, exit_on_request }
    }

    fn handle_request(&mut self, el: &ActiveEventLoop, msg: DialogMessageRequest) {
        if !self.ready {
            self.queued.push(msg);
            return;
        }
        let exit = matches!(msg, DialogMessageRequest::ExitEventLoop);
        let id = request_id(&msg);
        let keep = self.guarded(el, id, "handling a dialog request", |m, ws| m.handle_request(ws, msg)).unwrap_or(!exit);
        if !keep && self.exit_on_request {
            el.exit();
        }
    }

    /// Run `f` on the manager under `catch_unwind` (a panic closes only the affected dialog; the
    /// loop keeps running, like host mode). On a panic, dialog `id` (if known) is closed; if closing
    /// it panics too, the loop state can't be trusted any more and the loop exits (builder mode:
    /// open dialogs send `WindowClosed`; linux-direct: the handler switches to its dead state).
    fn guarded<R>(&mut self,
                  el: &ActiveEventLoop,
                  id: Option<usize>,
                  what: &str,
                  f: impl FnOnce(&mut Manager<T, WinitWin>, &mut WinitWs<'_>) -> R)
                  -> Option<R> {
        let mut ws = WinitWs { el, sb: self.sb.as_ref() };
        let manager = &mut self.manager;
        match catch_unwind(AssertUnwindSafe(|| f(manager, &mut ws))) {
            Ok(r) => return Some(r),
            Err(_) => error!("xdialog: {what} panicked"),
        }
        let id = id?;
        let closed = catch_unwind(AssertUnwindSafe(|| {
                                      manager.handle_request(&mut ws, DialogMessageRequest::CloseWindow(id));
                                  }));
        if closed.is_err() {
            error!("xdialog: closing dialog {id} after a panic panicked again; stopping the event loop");
            el.exit();
        }
        None
    }

    fn drain_inbox(&mut self, el: &ActiveEventLoop) {
        loop {
            let msg = match &self.inbox {
                Some(inbox) => match inbox.rx.try_recv() {
                    Ok(m) => m,
                    Err(_) => return,
                },
                None => return,
            };
            self.handle_request(el, msg);
        }
    }
}

impl<T: Theme> ApplicationHandler<UserEvent> for OwnLoopApp<T> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.ready {
            return;
        }
        match softbuffer::Context::new(el.owned_display_handle()) {
            Ok(c) => self.sb = Some(c),
            Err(e) => error!("xdialog: could not create the softbuffer context: {e}"),
        }
        self.ready = true;
        for msg in std::mem::take(&mut self.queued) {
            self.handle_request(el, msg);
        }
        if let Some(inbox) = &self.inbox {
            inbox.wake_pending.store(false, Ordering::SeqCst);
        }
        self.drain_inbox(el);
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Request(msg) => self.handle_request(el, msg),
            UserEvent::Wake => {
                if let Some(inbox) = &self.inbox {
                    inbox.wake_pending.store(false, Ordering::SeqCst);
                }
                if self.ready {
                    self.drain_inbox(el);
                }
            }
            UserEvent::Refresh => {
                self.guarded(el, None, "refreshing fonts/appearance", |m, ws| m.refresh(ws));
            }
            #[cfg(xd_test_hooks)]
            UserEvent::Remote(cmd) => {
                self.guarded(el, Some(cmd.id()), "a test-hook command", |m, ws| m.handle_remote(ws, cmd));
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        let Some(id) = self.manager.dialog_for(|w| w.id == window_id) else { return };
        match event {
            WindowEvent::RedrawRequested => {
                self.guarded(el, Some(id), "redraw", |m, ws| m.redraw(ws, id));
            }
            WindowEvent::ScaleFactorChanged { scale_factor, mut inner_size_writer } => {
                let size = self.guarded(el, Some(id), "handle_event", |m, ws| {
                                   m.handle_event(ws, id, HostEvent::ScaleFactorChanged { scale_factor });
                                   m.desired_physical_size(id, scale_factor as f32)
                               });
                // Keep the logical size: winit applies this physical size after the callback.
                if let Some(Some([w, h])) = size {
                    let _ = inner_size_writer.request_inner_size(PhysicalSize::new(w, h));
                }
            }
            other => {
                if let Some(ev) = translate(&other) {
                    self.guarded(el, Some(id), "handle_event", |m, ws| m.handle_event(ws, id, ev));
                }
            }
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        match self.guarded(el, None, "polling timers", |m, ws| m.poll_timers(ws, Instant::now())).flatten() {
            Some(t) => el.set_control_flow(ControlFlow::WaitUntil(t)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }

    fn exiting(&mut self, el: &ActiveEventLoop) {
        self.guarded(el, None, "closing the dialogs", |m, ws| m.close_all(ws));
    }
}

/// winit 0.30 window event -> version-neutral `HostEvent` (the same mapping as the host glue).
pub(crate) fn translate(ev: &WindowEvent) -> Option<HostEvent> {
    Some(match ev {
        WindowEvent::Resized(s) => HostEvent::Resized { width: s.width, height: s.height },
        WindowEvent::CursorMoved { position, .. } => HostEvent::CursorMoved { x: position.x, y: position.y },
        WindowEvent::CursorLeft { .. } => HostEvent::CursorLeft,
        WindowEvent::MouseInput { state, button, .. } => {
            let button = match button {
                winit::event::MouseButton::Left => MouseButton::Primary,
                winit::event::MouseButton::Right => MouseButton::Secondary,
                winit::event::MouseButton::Middle => MouseButton::Middle,
                _ => MouseButton::Other,
            };
            HostEvent::MouseButton { button, pressed: *state == ElementState::Pressed }
        }
        WindowEvent::MouseWheel { delta, .. } => HostEvent::MouseWheel { delta: match *delta {
                                                     MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines { x, y },
                                                     MouseScrollDelta::PixelDelta(p) => ScrollDelta::Pixels { x: p.x, y: p.y },
                                                 } },
        WindowEvent::Touch(t) => HostEvent::Touch { id: t.id,
                                                    phase: match t.phase {
                                                        winit::event::TouchPhase::Started => TouchPhase::Started,
                                                        winit::event::TouchPhase::Moved => TouchPhase::Moved,
                                                        winit::event::TouchPhase::Ended => TouchPhase::Ended,
                                                        winit::event::TouchPhase::Cancelled => TouchPhase::Cancelled,
                                                    },
                                                    x: t.location.x,
                                                    y: t.location.y },
        WindowEvent::KeyboardInput { event, is_synthetic, .. } => {
            if *is_synthetic {
                return None;
            }
            use winit::keyboard::{Key as WKey, NamedKey};
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
                WKey::Character(c) if c.as_str() == " " => Key::Space,
                _ => return None,
            };
            HostEvent::Key { key, pressed: event.state == ElementState::Pressed, repeat: event.repeat }
        }
        WindowEvent::ModifiersChanged(m) => {
            let s = m.state();
            HostEvent::Modifiers(Modifiers::new(s.shift_key(), s.control_key(), s.alt_key(), s.super_key()))
        }
        WindowEvent::Focused(f) => HostEvent::Focused(*f),
        WindowEvent::ThemeChanged(_) => HostEvent::ThemeChanged,
        WindowEvent::CloseRequested => HostEvent::CloseRequested,
        _ => return None,
    })
}

/// Real-window check of builder mode: run with
/// `cargo test --lib --features linux-egui,fluent-egui,_test-hooks live_builder -- --ignored`
/// (`XDIALOG_LIVE_THEME=fluent` for the Fluent theme; one event loop per process;
/// `XDIALOG_LIVE_OUT=<dir>` saves the captures). Windows are shown without activation in the
/// bottom-right corner (or at `XDIALOG_TEST_POS`), captured with `PrintWindow`, driven only through
/// the test-hook registry; the foreground window must never become one of ours.
#[cfg(all(test, windows, xd_test_hooks, xd_theme_linux, xd_theme_fluent))]
mod live_tests {
    use std::sync::mpsc::channel;
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
    use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetForegroundWindow, PW_RENDERFULLCONTENT};

    use super::super::manager::live::{self, LiveDialog, RemoteCmd};
    use super::*;
    use crate::backends::host_types::{HostEvent, Key};
    use crate::model::{XDialogIcon, XDialogOptions, XDialogResult};

    /// Client-area capture as (w, h, BGRA rows top-down): `PrintWindow(PW_RENDERFULLCONTENT)` of the
    /// whole window (`PW_CLIENTONLY` with full content returns a blank image), then cropped.
    fn capture(hwnd: isize) -> (usize, usize, Vec<u8>) {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::ClientToScreen;
        use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);
        // SAFETY: plain GDI calls on a live window owned by this process; every object is released.
        unsafe {
            let (mut wr, mut cr) = (RECT::default(), RECT::default());
            GetWindowRect(hwnd, &mut wr).unwrap();
            GetClientRect(hwnd, &mut cr).unwrap();
            let mut origin = POINT::default();
            let _ = ClientToScreen(hwnd, &mut origin);
            let (ww, wh) = ((wr.right - wr.left) as usize, (wr.bottom - wr.top) as usize);
            let (cw, ch) = ((cr.right - cr.left) as usize, (cr.bottom - cr.top) as usize);
            let (ox, oy) = ((origin.x - wr.left) as usize, (origin.y - wr.top) as usize);
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, ww as i32, wh as i32);
            let old = SelectObject(mem, bmp.into());
            let ok = PrintWindow(hwnd, mem, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT));
            assert!(ok.as_bool(), "PrintWindow failed");
            let mut bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32,
                                                                    biWidth: ww as i32,
                                                                    biHeight: -(wh as i32),
                                                                    biPlanes: 1,
                                                                    biBitCount: 32,
                                                                    biCompression: BI_RGB.0,
                                                                    ..Default::default() },
                                      ..Default::default() };
            let mut px = vec![0u8; ww * wh * 4];
            // GetDIBits requires the bitmap NOT to be selected into a DC.
            SelectObject(mem, old);
            GetDIBits(mem, bmp, 0, wh as u32, Some(px.as_mut_ptr().cast()), &mut bi, DIB_RGB_COLORS);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            let mut client = Vec::with_capacity(cw * ch * 4);
            for y in oy..(oy + ch).min(wh) {
                let row = (y * ww + ox) * 4;
                client.extend_from_slice(&px[row..row + cw.min(ww - ox) * 4]);
            }
            (cw, ch, client)
        }
    }

    /// Save a capture as PNG when `XDIALOG_LIVE_OUT` names a directory.
    fn save(img: &(usize, usize, Vec<u8>), name: &str) {
        let Ok(dir) = std::env::var("XDIALOG_LIVE_OUT") else { return };
        let rgba: Vec<u8> = img.2.chunks(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
        let _ = image::save_buffer(std::path::Path::new(&dir).join(name), &rgba, img.0 as u32, img.1 as u32, image::ColorType::Rgba8);
    }

    fn rgb_at(img: &(usize, usize, Vec<u8>), x: f32, y: f32) -> [u8; 3] {
        let i = (y as usize * img.0 + x as usize) * 4;
        [img.2[i + 2], img.2[i + 1], img.2[i]]
    }

    fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
        let t0 = Instant::now();
        while !f() {
            assert!(t0.elapsed() < Duration::from_secs(10), "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn info(id: usize) -> Option<LiveDialog> {
        live::snapshot().into_iter().find(|d| d.id == id)
    }

    /// Send a command and wait until the dialog presented a frame after it.
    fn cmd_and_frame(id: usize, cmd: RemoteCmd) -> LiveDialog {
        let before = info(id).unwrap().frames;
        assert!(live::send(cmd));
        wait_for("a frame", || info(id).is_some_and(|d| d.frames > before));
        std::thread::sleep(Duration::from_millis(30));
        info(id).unwrap()
    }

    fn opts(title: &str, buttons: &[&str]) -> XDialogOptions {
        XDialogOptions { title: title.into(),
                         main_instruction: "Live window check".into(),
                         message: "Rendered by the egui backend in a real window.".into(),
                         icon: XDialogIcon::Information,
                         buttons: buttons.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    #[ignore = "opens real (non-activated) windows"]
    fn live_builder_windows() {
        std::env::set_var("XDIALOG_TEST_NO_ACTIVATE", "1");
        if std::env::var_os("XDIALOG_TEST_POS").is_none() {
            // Bottom-right corner of the primary monitor. Not `offscreen`: PrintWindow returns a
            // blank client area for a window that is (mostly) off every monitor, because DWM keeps
            // no up-to-date redirection surface for it.
            use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
            // SAFETY: plain metrics queries.
            let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
            std::env::set_var("XDIALOG_TEST_POS", format!("{},{}", w - 520, h - 320));
        }
        let fluent = std::env::var("XDIALOG_LIVE_THEME").is_ok_and(|v| v == "fluent");
        // SAFETY: plain query.
        let fg_before = unsafe { GetForegroundWindow() }.0 as isize;
        let (tx, rx) = channel::<DialogMessageRequest>();

        let worker = std::thread::spawn(move || {
            /// Exit the loop even when an assertion fails (no hung test process).
            struct ExitOnDrop(std::sync::mpsc::Sender<DialogMessageRequest>);
            impl Drop for ExitOnDrop {
                fn drop(&mut self) {
                    let _ = self.0.send(DialogMessageRequest::ExitEventLoop);
                }
            }
            let _exit = ExitOnDrop(tx.clone());
            let mut ours = Vec::new();
            let fg_ok = |ours: &Vec<isize>| {
                // SAFETY: plain query.
                let fg = unsafe { GetForegroundWindow() }.0 as isize;
                assert!(!ours.contains(&fg), "a dialog window became the foreground window");
                fg
            };
            // A message dialog, shown from this (non-main) thread.
            let (ctx_, crx) = oneshot::channel();
            tx.send(DialogMessageRequest::ShowMessageWindow(9001, opts("xdialog live msg", &["Cancel", "OK"]), ctx_)).unwrap();
            let result = crx.recv().unwrap().unwrap();
            wait_for("live dialog", || info(9001).is_some_and(|d| d.frames > 0));
            let d = info(9001).unwrap();
            ours.push(d.raw_window);
            eprintln!("live: {d:?}");
            assert!(d.raw_window != 0 && d.size_px.0 >= 200 && d.size_px.1 >= 80, "{d:?}");
            fg_ok(&ours);

            // Hover changes the look once the fade (150 ms / WinUI 83 ms) has run on the real clock.
            let r = d.button_rects_px[0];
            let (cx, cy) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
            let (sx, sy) = (r[0] + 6.0, cy); // fill, left of the label
            let shot = |name: &str| {
                let img = capture(d.raw_window);
                save(&img, &format!("live_{}_{name}.png", if fluent { "fluent" } else { "linux" }));
                rgb_at(&img, sx, sy)
            };
            // DWM fills the redirection surface of a freshly shown, never-activated window a few
            // compositor frames later; until then PrintWindow returns black.
            wait_for("the first composited frame", || capture(d.raw_window).2.chunks(4).any(|p| p[..3] != [0, 0, 0]));
            let idle = shot("0_idle");
            cmd_and_frame(9001, RemoteCmd::Inject(9001, HostEvent::CursorMoved { x: cx as f64, y: cy as f64 }));
            std::thread::sleep(Duration::from_millis(400));
            let done = shot("1_hover");
            eprintln!("hover: idle {idle:?} hovered {done:?}");
            assert_ne!(done, idle, "hover changes the look");
            fg_ok(&ours);

            // An indeterminate progress dialog animates on the real clock.
            let (ctx_, crx) = oneshot::channel();
            tx.send(DialogMessageRequest::ShowProgressWindow(9002, opts("xdialog live progress", &[]), ctx_, None)).unwrap();
            crx.recv().unwrap().unwrap();
            tx.send(DialogMessageRequest::SetProgressIndeterminate(9002)).unwrap();
            wait_for("progress", || info(9002).is_some_and(|d| d.frames > 0));
            let p = info(9002).unwrap();
            ours.push(p.raw_window);
            std::thread::sleep(Duration::from_millis(100));
            let f0 = info(9002).unwrap().frames;
            let a = capture(p.raw_window);
            std::thread::sleep(Duration::from_millis(400));
            let b = capture(p.raw_window);
            let f1 = info(9002).unwrap().frames;
            eprintln!("progress frames in 400 ms: {}", f1 - f0);
            assert!(f1 - f0 >= 10, "indeterminate progress should repaint at ~60 fps ({} frames)", f1 - f0);
            assert_ne!(a.2, b.2, "indeterminate bar moved");
            fg_ok(&ours);

            // Keyboard: Escape closes the message dialog with WindowClosed; CloseWindow the other.
            assert!(live::send(RemoteCmd::Inject(9001, HostEvent::Key { key: Key::Escape, pressed: true, repeat: false })));
            assert_eq!(result.recv_timeout(Duration::from_secs(5)).unwrap(), XDialogResult::WindowClosed);
            tx.send(DialogMessageRequest::CloseWindow(9002)).unwrap();
            wait_for("windows closed", || info(9001).is_none() && info(9002).is_none());
            let fg = fg_ok(&ours);
            eprintln!("foreground before {fg_before:#x}, after {fg:#x}");
            tx.send(DialogMessageRequest::ExitEventLoop).unwrap();
        });

        let res = if fluent {
            run_builder(crate::backends::fluent_egui::FluentTheme::new(), rx, XDialogTheme::Light)
        } else {
            run_builder(crate::backends::linux_egui::LinuxTheme::new(), rx, XDialogTheme::Light)
        };
        assert!(res.is_ok(), "event loop failed to build");
        worker.join().unwrap();
    }
}
