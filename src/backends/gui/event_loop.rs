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
use crate::model::{XDialogBackend, XDialogTheme};

/// Build the event loop on this thread and install its request handler. The returned closure runs
/// the loop here until `ExitEventLoop`. `Err`: the loop could not be built (no display server, a
/// second winit loop, ...) or a handler is already installed.
pub(crate) fn start(backend: XDialogBackend, fallback: bool, xtheme: XDialogTheme) -> Result<Box<dyn FnOnce()>, String> {
    let event_loop = build()?;
    let proxy = event_loop.create_proxy();
    let waker = Box::new(move || {
        let _ = proxy.send_event(());
    });
    let mut rt = Runtime::install(backend, fallback, xtheme, waker).map_err(|e| e.to_string())?;
    Ok(Box::new(move || {
        if let Err(e) = event_loop.run_app(&mut rt) {
            error!("xdialog: event loop error: {e}");
        }
    }))
}

/// Build the event loop: `with_any_thread(true)` (tests and `XDialogBuilder` run on arbitrary
/// threads), Windows `with_dpi_aware(false)` (the thread sets per-monitor-v2 itself; a library
/// must not change process DPI state). A panic inside `build()` is caught.
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
