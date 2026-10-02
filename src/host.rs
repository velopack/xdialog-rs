//! Run xdialog's dialogs inside a winit 0.30 event loop your application owns (feature
//! `winit-host`).
//!
//! winit allows one event loop per process, so an application that runs its own can't also let
//! [`XDialogBuilder::run`](crate::XDialogBuilder::run) run xdialog's. Instead, create an
//! [`XDialogHost`] on the event-loop thread with
//! [`XDialogBuilder::into_host`](crate::XDialogBuilder::into_host) and, for each run of your loop,
//! wrap your `ApplicationHandler` with [`XDialogHost::wrap`]; the wrapper, a [`HostedApp`], is what
//! you pass to `run_app_on_demand` (or `pump_app_events`). An application that runs its loop once,
//! with `run_app`, can skip the host:
//! [`XDialogBuilder::into_host_app`](crate::XDialogBuilder::into_host_app) returns an
//! [`XDialogApp`] that owns both your app and the host. Either way your handler needs no xdialog
//! code:
//!
//! - `window_event` for xdialog's dialog windows is handled and not forwarded; everything else
//!   reaches your app. As usual in winit, ignore `WindowId`s you don't own.
//! - `about_to_wait` runs your app first, then creates, updates and closes dialogs. When a dialog
//!   needs a frame, xdialog wakes the loop no later than that: `Wait` becomes a `WaitUntil`, an
//!   earlier `WaitUntil` of yours is kept, `Poll` is never touched. Your app always sees its own
//!   control flow and `StartCause`s: xdialog's deadline is undone before `new_events` is forwarded.
//! - `exiting` closes the dialogs (blocked callers get `WindowClosed`) before it is forwarded, so
//!   your `exiting` can join threads that were waiting on a dialog. A [`HostedApp`] keeps the host
//!   serving for the next run; an [`XDialogApp`] shuts it down (later calls `NoBackendAvailable`).
//! - The waker's event arrives in your `user_event` as your own `T`: ignore it.
//! - Every other callback is forwarded unchanged.
//!
//! xdialog creates its dialog windows itself through your `ActiveEventLoop`. [`winit`] is
//! re-exported: use it (or the same winit 0.30 version) for your loop. `examples/winit_host.rs` in
//! the repository is a complete host; `examples/winit_host_on_demand.rs` runs its loop several
//! times.
//!
//! The backend is chosen as for [`XDialogBuilder::run`](crate::XDialogBuilder::run), except that
//! AppKit needs its own loop: an explicit `AppKit` fails with `NoBackendAvailable`.
//!
//! Dialog functions work from any thread; the event-loop thread is xdialog's UI thread (see
//! [Threads](crate#threads)). There, ask with [`show_message`](crate::show_message) and check
//! the returned proxy's [`try_result`](crate::MessageDialogProxy::try_result) in `about_to_wait`:
//! xdialog wakes the loop when the answer arrives.
//!
//! The dialogs get the event-loop thread's DPI awareness. winit's default
//! (`with_dpi_aware(true)`) makes the process per-monitor-v2 aware on Windows; a host that opts
//! out gets bitmap-scaled dialogs on high-DPI monitors.
//!
//! # Several runs of one loop
//!
//! `run_app_on_demand` runs the loop again, with any app value: winit emits `exiting` at the end
//! of every run and carries no window over to the next. The host outlives the runs: create it
//! once, wrap each run's app, and drop it after the last run. (`pump_app_events` drives a run
//! too, but once it returned `PumpStatus::Exit`, start the next run with `run_app_on_demand`:
//! winit 0.30's `pump_app_events` doesn't clear the exit on X11/Wayland.)
//!
//! - A run ends with every dialog closed (`WindowClosed`), like a `run_app` host exits.
//! - Between runs, requests (from other threads, or from the event-loop thread after the loop
//!   returned) are queued and the waker is called; the next run serves them in its first
//!   `about_to_wait`, so a blocking call made between runs from another thread blocks until then
//!   (on the event-loop thread it fails with `BlockingCallOnUiThread`, as always). Requests the ending
//!   run didn't get to (one made from `exiting`, or during its last iteration) queue the same way.
//! - [`XDialogHost::shutdown`] (or dropping the host) ends xdialog for the process at any time:
//!   open dialogs close, queued requests and later calls get `NoBackendAvailable`. Closed dialog
//!   windows report `Destroyed` when the thread next pumps messages, possibly in a later run; the
//!   host remembers them (shut down or not) and a wrapper of that host keeps those events from
//!   your app. After the host is dropped they reach your app like any foreign window's.
//!
//! ```rust,no_run
//! use xdialog::host::winit::application::ApplicationHandler;
//! use xdialog::host::winit::event::WindowEvent;
//! use xdialog::host::winit::event_loop::{ActiveEventLoop, EventLoop};
//! use xdialog::host::winit::platform::run_on_demand::EventLoopExtRunOnDemand;
//! use xdialog::host::winit::window::WindowId;
//!
//! struct App; // your windows ...
//!
//! impl ApplicationHandler for App {
//!     fn resumed(&mut self, _el: &ActiveEventLoop) { /* create your windows */ }
//!
//!     fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, _event: WindowEvent) {
//!         // ... your windows only
//!         el.exit();
//!     }
//! }
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut event_loop = EventLoop::new()?;
//!     let proxy = event_loop.create_proxy();
//!     let mut host = xdialog::XDialogBuilder::new().into_host(move || {
//!                                                      let _ = proxy.send_event(());
//!                                                  })?;
//!     std::thread::spawn(|| xdialog::show_message_info_ok("Hosted", "Hi", "From a worker thread."));
//!     for _ in 0..2 {
//!         let mut app = App; // a new app value for every run
//!         event_loop.run_app_on_demand(&mut host.wrap(&mut app))?;
//!     }
//!     Ok(())
//! }
//! ```

use std::time::Instant;

pub use winit;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

use crate::backends::gui::runtime::Runtime;
use crate::channel::WakeFn;
use crate::model::{XDialogBackend, XDialogTheme};
use crate::XDialogError;

/// xdialog's dialogs in an event loop your application owns, across any number of runs of that
/// loop (see the [module docs](self)). Created by
/// [`XDialogBuilder::into_host`](crate::XDialogBuilder::into_host) on the event-loop thread, once
/// per process; not `Send`.
///
/// [`wrap`](Self::wrap) adds the dialogs to the app of one run. Dropping the host (or
/// [`shutdown`](Self::shutdown)) closes every open dialog like a run's `exiting` does, and ends
/// xdialog for the process: queued requests and later calls get `NoBackendAvailable`. The windows
/// are hidden at once; winit releases them when the thread next pumps messages.
pub struct XDialogHost {
    /// `None` after `shutdown`.
    rt: Option<Runtime>,
    /// After `shutdown`: the dialogs' windows, until their `Destroyed` (never forwarded).
    closed: Vec<WindowId>,
    /// The app's control flow, while ours replaces it (from `about_to_wait` to `new_events`).
    flow: Option<ControlFlow>,
    #[cfg(feature = "_test-hooks")]
    last_flow: Option<ControlFlow>,
}

impl XDialogHost {
    pub(crate) fn new(requested: XDialogBackend, xtheme: XDialogTheme, waker: WakeFn) -> Result<Self, XDialogError> {
        let (backend, fallback) = match crate::backends::resolve(requested) {
            // AppKit needs its own loop.
            Some((XDialogBackend::AppKit, _)) | None => return Err(XDialogError::NoBackendAvailable),
            Some(chosen) => chosen,
        };
        Ok(XDialogHost { rt: Some(Runtime::install(backend, fallback, xtheme, waker)?),
                         closed: Vec::new(),
                         flow: None,
                         #[cfg(feature = "_test-hooks")]
                         last_flow: None })
    }

    /// Your `ApplicationHandler` with xdialog's dialogs added, for one run of your event loop:
    /// pass the wrapper to `run_app_on_demand` (or `pump_app_events`). `app` is usually `&mut`
    /// your app (winit implements `ApplicationHandler` for `&mut A`), but any value works. Wrap
    /// again for the next run, with the same or another app.
    pub fn wrap<A>(&mut self, app: A) -> HostedApp<'_, A> {
        HostedApp { host: self, app }
    }

    /// End xdialog for the process now (dropping the host does the same): every open dialog
    /// closes (`WindowClosed`), queued requests and later calls get `NoBackendAvailable`. A
    /// wrapper of this host still keeps the closed windows' late events from your app; everything
    /// else is forwarded unchanged. Calling it again does nothing.
    pub fn shutdown(&mut self) {
        if let Some(rt) = self.rt.take() {
            self.closed = rt.window_ids();
        }
        // `flow` is kept: called from a callback of a running loop, the next `new_events` still
        // restores the app's control flow.
    }

    /// Whether [`shutdown`](Self::shutdown) has run.
    pub fn is_shut_down(&self) -> bool {
        self.rt.is_none()
    }

    // ---- the wrappers' behaviour (`HostedApp` and `XDialogApp` share it) ----

    fn new_events<T: 'static>(&mut self, app: &mut impl ApplicationHandler<T>, el: &ActiveEventLoop, mut cause: StartCause) {
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
        app.new_events(el, cause);
    }

    fn window_event<T: 'static>(&mut self, app: &mut impl ApplicationHandler<T>, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
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
            app.window_event(el, id, event);
        }
    }

    fn about_to_wait<T: 'static>(&mut self, app: &mut impl ApplicationHandler<T>, el: &ActiveEventLoop) {
        // The app first: dialogs it requests here appear in this iteration.
        app.about_to_wait(el);
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

    /// A run of the loop is ending: close every dialog (`WindowClosed`), keep serving. Queued
    /// requests stay queued for the next run (their wake-up is already on its way or, once the
    /// next run drains the queue, sent again).
    fn end_run(&mut self) {
        if let Some(rt) = &mut self.rt {
            rt.close_all();
        }
        // Never restore this run's flow onto the next run.
        self.flow = None;
    }
}

/// Your `ApplicationHandler` with xdialog's dialogs added, for one run of your event loop (see
/// the [module docs](self)). Created by [`XDialogHost::wrap`]; borrows the host, so it can't
/// outlive it and no two can exist at once.
///
/// `exiting` closes every open dialog and forwards; the host keeps serving (the next run's
/// wrapper shows the dialogs requested since). Dropping the wrapper without `exiting` (the loop
/// is still running, as with `pump_app_events`) leaves the dialogs open: wrap again to go on.
pub struct HostedApp<'h, A> {
    host: &'h mut XDialogHost,
    app: A,
}

impl<A> HostedApp<'_, A> {
    /// Your app.
    pub fn inner(&self) -> &A {
        &self.app
    }

    /// Your app. Calling its `ApplicationHandler` methods directly bypasses xdialog.
    pub fn inner_mut(&mut self) -> &mut A {
        &mut self.app
    }

    /// Your app back; the dialogs stay open.
    pub fn into_inner(self) -> A {
        self.app
    }

    /// The host.
    pub fn host(&self) -> &XDialogHost {
        self.host
    }

    /// The host.
    pub fn host_mut(&mut self) -> &mut XDialogHost {
        self.host
    }
}

impl<T: 'static, A: ApplicationHandler<T>> ApplicationHandler<T> for HostedApp<'_, A> {
    fn new_events(&mut self, el: &ActiveEventLoop, cause: StartCause) {
        self.host.new_events(&mut self.app, el, cause);
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.app.resumed(el);
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: T) {
        // xdialog's wake-up is one of these; `about_to_wait` (always next) serves the requests.
        self.app.user_event(el, event);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.host.window_event(&mut self.app, el, id, event);
    }

    fn device_event(&mut self, el: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        self.app.device_event(el, id, event);
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.host.about_to_wait(&mut self.app, el);
    }

    fn suspended(&mut self, el: &ActiveEventLoop) {
        self.app.suspended(el);
    }

    fn exiting(&mut self, el: &ActiveEventLoop) {
        // Close the dialogs first: the app may join threads blocked on one.
        self.host.end_run();
        self.app.exiting(el);
    }

    fn memory_warning(&mut self, el: &ActiveEventLoop) {
        self.app.memory_warning(el);
    }
}

/// Your `ApplicationHandler` with xdialog's dialogs added, owning its [`XDialogHost`]: for a loop
/// that runs once, with `run_app` (see the [module docs](self)). Created by
/// [`XDialogBuilder::into_host_app`](crate::XDialogBuilder::into_host_app) on the event-loop
/// thread; not `Send`.
///
/// `exiting` shuts the host down before it is forwarded: blocked callers get `WindowClosed`,
/// later calls `NoBackendAvailable`. Dropping it (or [`into_inner`](Self::into_inner)) closes
/// every open dialog the same way. The windows are hidden at once; winit releases them when the
/// thread next pumps messages.
pub struct XDialogApp<A> {
    app: A,
    host: XDialogHost,
}

impl<A> XDialogApp<A> {
    pub(crate) fn new(app: A, host: XDialogHost) -> Self {
        XDialogApp { app, host }
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

    /// The host.
    pub fn host(&self) -> &XDialogHost {
        &self.host
    }

    /// The host.
    pub fn host_mut(&mut self) -> &mut XDialogHost {
        &mut self.host
    }
}

impl<T: 'static, A: ApplicationHandler<T>> ApplicationHandler<T> for XDialogApp<A> {
    fn new_events(&mut self, el: &ActiveEventLoop, cause: StartCause) {
        self.host.new_events(&mut self.app, el, cause);
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.app.resumed(el);
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: T) {
        self.app.user_event(el, event);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.host.window_event(&mut self.app, el, id, event);
    }

    fn device_event(&mut self, el: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        self.app.device_event(el, id, event);
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.host.about_to_wait(&mut self.app, el);
    }

    fn suspended(&mut self, el: &ActiveEventLoop) {
        self.app.suspended(el);
    }

    fn exiting(&mut self, el: &ActiveEventLoop) {
        // End xdialog first: the app may join threads blocked on a dialog, and a dialog call
        // they make afterwards must fail fast rather than wait for a run that never comes.
        self.host.shutdown();
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
    /// Button rects in logical px `[x, y, w, h]`, by API index.
    pub button_rects: Vec<[f64; 4]>,
    /// Frames presented so far.
    pub frames: u64,
    /// The accessibility tree, one node per line.
    pub a11y: String,
}

#[cfg(feature = "_test-hooks")]
#[doc(hidden)]
impl XDialogHost {
    /// The open drawn dialogs.
    pub fn test_dialogs(&self) -> Vec<LiveDialog> {
        self.rt.as_ref().map_or_else(Vec::new, Runtime::test_dialogs)
    }

    /// Apply an input event (logical px) to dialog `id` now; its frame follows in `about_to_wait`.
    pub fn test_inject(&mut self, id: usize, event: crate::__test::Event) {
        if let Some(rt) = &mut self.rt {
            rt.test_inject(id, event);
        }
    }

    /// The control flow the loop waits with after the last `about_to_wait` (`None`: none yet).
    pub fn test_control_flow(&self) -> Option<ControlFlow> {
        self.last_flow
    }
}

#[cfg(feature = "_test-hooks")]
#[doc(hidden)]
impl<A> HostedApp<'_, A> {
    /// See [`XDialogHost::test_dialogs`].
    pub fn test_dialogs(&self) -> Vec<LiveDialog> {
        self.host.test_dialogs()
    }

    /// See [`XDialogHost::test_inject`].
    pub fn test_inject(&mut self, id: usize, event: crate::__test::Event) {
        self.host.test_inject(id, event);
    }

    /// See [`XDialogHost::test_control_flow`].
    pub fn test_control_flow(&self) -> Option<ControlFlow> {
        self.host.test_control_flow()
    }
}

#[cfg(feature = "_test-hooks")]
#[doc(hidden)]
impl<A> XDialogApp<A> {
    /// See [`XDialogHost::test_dialogs`].
    pub fn test_dialogs(&self) -> Vec<LiveDialog> {
        self.host.test_dialogs()
    }

    /// See [`XDialogHost::test_inject`].
    pub fn test_inject(&mut self, id: usize, event: crate::__test::Event) {
        self.host.test_inject(id, event);
    }

    /// See [`XDialogHost::test_control_flow`].
    pub fn test_control_flow(&self) -> Option<ControlFlow> {
        self.host.test_control_flow()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::{show_message, XDialogOptions};

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

    /// No event loop: the host's state around the request queue. (It installs the process's
    /// request handler, which every unit test of this binary shares: none may rely on one being
    /// absent, such as expecting `NotInitialized` from a dialog call.)
    #[test]
    fn runs_keep_the_queue_and_shutdown_rejects_it() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = wakes.clone();
        // Windows: Fluent without the TaskDialog fallback; elsewhere the platform's drawn look.
        let backend = if cfg!(windows) { XDialogBackend::Fluent } else { XDialogBackend::Auto };
        let waker = move || {
            w.fetch_add(1, Ordering::SeqCst);
        };
        let mut host = XDialogHost::new(backend, XDialogTheme::SystemDefault, Box::new(waker)).expect("host");
        assert!(matches!(XDialogHost::new(backend, XDialogTheme::SystemDefault, Box::new(|| {})), Err(XDialogError::SystemError(_))),
                "one host per process");
        assert!(!host.is_shut_down());

        // Between runs (nothing drains): requests queue, the waker is called once.
        let ask = |title: &str| show_message(XDialogOptions { title: title.into(), ..Default::default() });
        let (a, b) = (ask("a"), ask("b"));
        assert!(a.try_result().is_none() && b.try_result().is_none(), "queued, not answered");
        assert_eq!(wakes.load(Ordering::SeqCst), 1, "wake-ups coalesce until the next drain");

        // A run ends: the queue is kept for the next one.
        host.end_run();
        assert!(a.try_result().is_none() && b.try_result().is_none());
        assert!(!host.is_shut_down());

        // Shutdown: the queue is rejected, later requests fail fast; a second shutdown is a no-op.
        host.shutdown();
        assert!(host.is_shut_down());
        assert!(matches!(a.try_result(), Some(Err(XDialogError::NoBackendAvailable))));
        assert!(matches!(b.try_result(), Some(Err(XDialogError::NoBackendAvailable))));
        assert!(matches!(ask("c").try_result(), Some(Err(XDialogError::NoBackendAvailable))));
        host.shutdown();
        assert!(host.is_shut_down());
        assert_eq!(wakes.load(Ordering::SeqCst), 1, "no wake-ups after shutdown");
    }
}
