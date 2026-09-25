//! Run xdialog's dialogs inside a winit 0.30 event loop your application owns (feature
//! `winit-host`).
//!
//! winit allows one event loop per process, so an application that runs its own can't also let
//! [`XDialogBuilder::run`](crate::XDialogBuilder::run) run xdialog's. Instead, wrap your
//! `ApplicationHandler` with [`XDialogBuilder::into_host_app`](crate::XDialogBuilder::into_host_app)
//! on the event-loop thread and pass the wrapper, an [`XDialogApp`], to `run_app` (or
//! `pump_app_events`). Your handler needs no xdialog code:
//!
//! - `window_event` for xdialog's dialog windows is handled and not forwarded; everything else
//!   reaches your app. As usual in winit, ignore `WindowId`s you don't own.
//! - `about_to_wait` runs your app first, then creates, updates and closes dialogs. When a dialog
//!   needs a frame, xdialog wakes the loop no later than that: `Wait` becomes a `WaitUntil`, an
//!   earlier `WaitUntil` of yours is kept, `Poll` is never touched. Your app always sees its own
//!   control flow and `StartCause`s: xdialog's deadline is undone before `new_events` is forwarded.
//! - `exiting` closes the dialogs (blocked callers get `WindowClosed`, later calls
//!   `NoBackendAvailable`) before it is forwarded, so your `exiting` can join threads that were
//!   waiting on a dialog.
//! - The waker's event arrives in your `user_event` as your own `T`: ignore it.
//! - Every other callback is forwarded unchanged.
//!
//! xdialog creates its dialog windows itself through your `ActiveEventLoop`. [`winit`] is
//! re-exported: use it (or the same winit 0.30 version) for your loop. `examples/winit_host.rs` in
//! the repository is a complete host.
//!
//! The backend is chosen as for [`XDialogBuilder::run`](crate::XDialogBuilder::run), except that
//! AppKit needs its own loop: on macOS `Auto` uses the `Ubuntu` look, and an explicit `AppKit`
//! fails with `NoBackendAvailable`.
//!
//! Dialog functions work from any thread; the event-loop thread is xdialog's UI thread (see
//! [Threads](crate#threads)).
//!
//! The dialogs get the event-loop thread's DPI awareness. winit's default
//! (`with_dpi_aware(true)`) makes the process per-monitor-v2 aware on Windows; a host that opts
//! out gets bitmap-scaled dialogs on high-DPI monitors.
//!
//! ```rust,no_run
//! use xdialog::host::winit::application::ApplicationHandler;
//! use xdialog::host::winit::event::WindowEvent;
//! use xdialog::host::winit::event_loop::{ActiveEventLoop, EventLoop};
//! use xdialog::host::winit::window::WindowId;
//!
//! struct App; // your windows ...
//!
//! impl ApplicationHandler for App {
//!     fn resumed(&mut self, _el: &ActiveEventLoop) { /* create your windows */ }
//!
//!     fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, _event: WindowEvent) {
//!         // ... your windows only
//!     }
//! }
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let event_loop = EventLoop::new()?;
//!     let proxy = event_loop.create_proxy();
//!     let mut app = xdialog::XDialogBuilder::new().into_host_app(App, move || {
//!                                                     let _ = proxy.send_event(());
//!                                                 })?;
//!     std::thread::spawn(|| xdialog::show_message_info_ok("Hosted", "Hi", "From a worker thread."));
//!     event_loop.run_app(&mut app)?;
//!     Ok(())
//! }
//! ```

use std::time::Instant;

pub use winit;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

use crate::backends::egui_core::runtime::Runtime;
use crate::channel::WakeFn;
use crate::model::{XDialogBackend, XDialogTheme};
use crate::XDialogError;

/// Your `ApplicationHandler` with xdialog's dialogs added (see the [module docs](self)). Created
/// by [`XDialogBuilder::into_host_app`](crate::XDialogBuilder::into_host_app) on the event-loop
/// thread; not `Send`.
///
/// Dropping it (or [`into_inner`](Self::into_inner)) closes every open dialog like `exiting`
/// does. The windows are hidden at once; winit releases them when the thread next pumps
/// messages.
pub struct XDialogApp<A> {
    app: A,
    /// `None` after `exiting`.
    rt: Option<Runtime>,
    /// After `exiting`: the dialogs' windows, until their `Destroyed` (never forwarded).
    closed: Vec<WindowId>,
    /// The host's control flow, while ours replaces it (from `about_to_wait` to `new_events`).
    flow: Option<ControlFlow>,
    #[cfg(feature = "_test-hooks")]
    last_flow: Option<ControlFlow>,
}

impl<A> XDialogApp<A> {
    pub(crate) fn new(app: A, requested: XDialogBackend, xtheme: XDialogTheme, waker: WakeFn) -> Result<Self, XDialogError> {
        let (backend, fallback) = match crate::backends::resolve(requested) {
            // AppKit needs its own loop: `Auto` on macOS gets an egui look instead.
            Some((XDialogBackend::AppKit, _)) if requested == XDialogBackend::Auto => (XDialogBackend::Ubuntu, false),
            Some((XDialogBackend::AppKit, _)) | None => return Err(XDialogError::NoBackendAvailable),
            Some(chosen) => chosen,
        };
        Ok(XDialogApp { app,
                        rt: Some(Runtime::install(backend, fallback, xtheme, waker)?),
                        closed: Vec::new(),
                        flow: None,
                        #[cfg(feature = "_test-hooks")]
                        last_flow: None })
    }

    /// Your app.
    pub fn inner(&self) -> &A {
        &self.app
    }

    /// Your app. Calling its `ApplicationHandler` methods directly bypasses xdialog.
    pub fn inner_mut(&mut self) -> &mut A {
        &mut self.app
    }

    /// Your app back; closes xdialog's dialogs.
    pub fn into_inner(self) -> A {
        self.app
    }
}

impl<T: 'static, A: ApplicationHandler<T>> ApplicationHandler<T> for XDialogApp<A> {
    fn new_events(&mut self, el: &ActiveEventLoop, mut cause: StartCause) {
        // Undo our deadline so the app sees its own flow, and a wake-up for it as a cancelled wait.
        if let Some(host) = self.flow.take() {
            el.set_control_flow(host);
            if let StartCause::ResumeTimeReached { start, .. } | StartCause::WaitCancelled { start, .. } = cause {
                cause = match host {
                    ControlFlow::WaitUntil(h) if Instant::now() >= h => StartCause::ResumeTimeReached { start, requested_resume: h },
                    ControlFlow::WaitUntil(h) => StartCause::WaitCancelled { start, requested_resume: Some(h) },
                    _ => StartCause::WaitCancelled { start, requested_resume: None },
                };
            }
        }
        self.app.new_events(el, cause);
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.app.resumed(el);
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: T) {
        // xdialog's wake-up is one of these; `about_to_wait` (always next) serves the requests.
        self.app.user_event(el, event);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let ours = match &mut self.rt {
            Some(rt) => rt.window_event(id, &event),
            None => {
                let ours = self.closed.contains(&id);
                if matches!(event, WindowEvent::Destroyed) {
                    self.closed.retain(|c| *c != id);
                }
                ours
            }
        };
        if !ours {
            self.app.window_event(el, id, event);
        }
    }

    fn device_event(&mut self, el: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        self.app.device_event(el, id, event);
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // The app first: dialogs it requests here appear in this iteration.
        self.app.about_to_wait(el);
        if el.exiting() {
            return; // `exiting` closes the dialogs; don't open new ones
        }
        if let Some(next) = self.rt.as_mut().and_then(|rt| rt.about_to_wait(el)) {
            let host = el.control_flow();
            let merged = merge(host, next);
            if merged != host {
                el.set_control_flow(merged);
                self.flow = Some(host);
            }
        }
        #[cfg(feature = "_test-hooks")]
        {
            self.last_flow = Some(el.control_flow());
        }
    }

    fn suspended(&mut self, el: &ActiveEventLoop) {
        self.app.suspended(el);
    }

    fn exiting(&mut self, el: &ActiveEventLoop) {
        // Close the dialogs first: the app may join threads blocked on one.
        if let Some(rt) = self.rt.take() {
            self.closed = rt.window_ids();
        }
        self.app.exiting(el);
    }

    fn memory_warning(&mut self, el: &ActiveEventLoop) {
        self.app.memory_warning(el);
    }
}

/// The host's control flow, woken no later than `next`.
fn merge(host: ControlFlow, next: Instant) -> ControlFlow {
    match host {
        ControlFlow::Poll => host,
        ControlFlow::WaitUntil(t) if t <= next => host,
        _ => ControlFlow::WaitUntil(next),
    }
}

/// A live dialog window (test hooks).
#[cfg(feature = "_test-hooks")]
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct LiveDialog {
    /// Dialog id.
    pub id: usize,
    /// Window title.
    pub title: String,
    /// Button rects in points `[x, y, w, h]`, by API index.
    pub button_rects: Vec<[f32; 4]>,
    /// Frames presented so far.
    pub frames: u64,
}

#[cfg(feature = "_test-hooks")]
#[doc(hidden)]
impl<A> XDialogApp<A> {
    /// The open egui dialogs.
    pub fn test_dialogs(&self) -> Vec<LiveDialog> {
        self.rt.as_ref().map_or_else(Vec::new, Runtime::test_dialogs)
    }

    /// Apply an input event (points) to dialog `id` now; its frame follows in `about_to_wait`.
    pub fn test_inject(&mut self, id: usize, event: egui::Event) {
        if let Some(rt) = &mut self.rt {
            rt.test_inject(id, event);
        }
    }

    /// The control flow the loop waits with after the last `about_to_wait` (`None`: none yet).
    pub fn test_control_flow(&self) -> Option<ControlFlow> {
        self.last_flow
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn merge_control_flow() {
        let next = Instant::now() + Duration::from_millis(16);
        let (earlier, later) = (next - Duration::from_millis(5), next + Duration::from_secs(1));
        assert_eq!(merge(ControlFlow::Poll, next), ControlFlow::Poll);
        assert_eq!(merge(ControlFlow::Wait, next), ControlFlow::WaitUntil(next));
        assert_eq!(merge(ControlFlow::WaitUntil(earlier), next), ControlFlow::WaitUntil(earlier));
        assert_eq!(merge(ControlFlow::WaitUntil(next), next), ControlFlow::WaitUntil(next));
        assert_eq!(merge(ControlFlow::WaitUntil(later), next), ControlFlow::WaitUntil(next));
    }
}
