//! Builder mode: xdialog's own winit 0.30 event loop on the caller's thread (the user's main
//! thread, `XDialogBuilder::run`), serving the request channel with a [`Runtime`].
//!
//! winit allows one event loop per process (it never resets its "created" flag on desktop), so
//! this loop is the process's winit loop; applications that run their own use `into_host_app`.

use std::panic::{catch_unwind, AssertUnwindSafe};

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

use super::runtime::Runtime;
use crate::channel::WakeFn;
use crate::model::{XDialogBackend, XDialogTheme};

/// Build the event loop on this thread and install its request handler. The returned closure runs
/// the loop here until `ExitEventLoop`. `Err`: the loop could not be built (no display server, a
/// second winit loop, ...) or a handler is already installed.
///
/// macOS: `Err` only for an installed handler; the loop is built when the first dialog is
/// requested, not here (a build failure then answers the requests with `NoBackendAvailable`).
/// Building it connects to the window server, which registers the process with LaunchServices: an
/// executable inside another app's bundle (an updater in `Contents/MacOS`) then checks in as a
/// second instance of that app and can show in the Dock and take focus. Most runs of such tools
/// show no dialog, so they stay invisible (as the AppKit backend does).
pub(crate) fn start(backend: XDialogBackend, fallback: bool, xtheme: XDialogTheme) -> Result<Box<dyn FnOnce()>, String> {
    #[cfg(target_os = "macos")]
    {
        let mut rt = Runtime::install(backend, fallback, xtheme, Box::new(|| {})).map_err(|e| e.to_string())?;
        Ok(Box::new(move || {
            let Some(first) = rt.wait_for_first_dialog() else { return };
            let event_loop = match build() {
                Ok(el) => el,
                Err(e) => {
                    // Dropping `rt` answers the queued and later requests with NoBackendAvailable.
                    warn!("xdialog: drawn backend unavailable ({e})");
                    crate::channel::reject(first);
                    return;
                }
            };
            rt.attach(first, waker(&event_loop));
            run(event_loop, &mut rt);
        }))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let event_loop = build()?;
        let mut rt = Runtime::install(backend, fallback, xtheme, waker(&event_loop)).map_err(|e| e.to_string())?;
        Ok(Box::new(move || run(event_loop, &mut rt)))
    }
}

/// A waker that makes `el` iterate (its `user_event` is winit's default no-op).
fn waker(el: &EventLoop<()>) -> WakeFn {
    let proxy = el.create_proxy();
    Box::new(move || {
        let _ = proxy.send_event(());
    })
}

/// Run the loop here until `ExitEventLoop`.
fn run(el: EventLoop<()>, rt: &mut Runtime) {
    if let Err(e) = el.run_app(rt) {
        error!("xdialog: event loop error: {e}");
    }
}

/// Build the event loop: `with_any_thread(true)` (tests and `XDialogBuilder` run on arbitrary
/// threads), Windows `with_dpi_aware(false)` (the thread sets per-monitor-v2 itself; a library
/// must not change process DPI state), macOS an accessory app (no Dock icon or menu bar, like
/// CFUserNotification alerts; windows still take focus) without winit's default menu.
fn build() -> Result<EventLoop<()>, String> {
    let built = catch_unwind(AssertUnwindSafe(|| {
                                 let mut b = EventLoop::<()>::with_user_event();
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
                                 #[cfg(target_os = "macos")]
                                 {
                                     use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
                                     b.with_activation_policy(ActivationPolicy::Accessory);
                                     b.with_default_menu(false);
                                     b.with_activate_ignoring_other_apps(!super::runtime::no_activate());
                                 }
                                 b.build()
                             }));
    match built {
        Ok(Ok(el)) => Ok(el),
        Ok(Err(e)) => Err(format!("could not create the event loop: {e}")),
        Err(_) => Err("building the event loop panicked".into()),
    }
}

/// The wake-up (`user_event`, winit's default no-op) is followed by `about_to_wait`.
impl ApplicationHandler<()> for Runtime {
    fn resumed(&mut self, _el: &ActiveEventLoop) {}

    fn window_event(&mut self, _el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        Runtime::window_event(self, id, &event);
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let next = Runtime::about_to_wait(self, el);
        if self.exit {
            // winit (Windows) checks `exit` only after the wait that follows `about_to_wait`:
            // don't wait.
            el.set_control_flow(ControlFlow::Poll);
            el.exit();
        } else {
            el.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
        }
    }
}
