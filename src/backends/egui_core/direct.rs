//! linux-direct mode: a lazily started, persistent UI thread with its own winit 0.30 loop.
//!
//! - [`init_linux_direct`] only installs a [`DirectHandler`]: no thread, no event loop. winit's
//!   one-loop-per-process slot is untouched until the first dialog request.
//! - The first request spawns the `xdialog-ui` thread. It builds the loop with
//!   [`build_event_loop`], marks itself as a UI thread (blocking-call guard), publishes
//!   the loop proxy and runs [`OwnLoopApp`] with an [`Inbox`] forever (`exit_on_request = false`).
//! - Requests go through an mpsc channel; a [`UserEvent::Wake`] is sent only when `wake_pending`
//!   was clear. The loop clears the flag BEFORE draining, and drains everything queued before the
//!   proxy existed in `resumed`, so no request is ever stranded.
//! - If the loop can't be built, or `run_app` unwinds or returns, the handler switches to a "dead"
//!   state: every later request is answered immediately with the error (creation requests fail,
//!   updates are ignored), and whatever was queued is answered too. Callers never hang.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, Once, OnceLock};

use winit::event_loop::EventLoopProxy;

use super::own_loop::{build_event_loop, Inbox, LoopBuildError, OwnLoopApp, UiThreadMark, UserEvent};
use crate::backends::answer_with_error;
use crate::channel::DialogRequestHandler;
use crate::model::{DialogMessageRequest, XDialogTheme};
use crate::XDialogError;

const RECREATION_MSG: &str = "xdialog linux-direct: this process already created a winit 0.30 event loop (winit allows one per \
                              process); use the `winit-host` feature and integrate with it instead";

/// Why the UI thread can no longer show dialogs.
#[derive(Clone, Debug)]
enum Dead {
    /// No display server (or the loop could not be built for another reason).
    NoBackend,
    /// A permanent error with a message for the caller.
    Error(String),
}

impl Dead {
    fn error(&self) -> XDialogError {
        match self {
            Dead::NoBackend => XDialogError::NoBackendAvailable,
            Dead::Error(s) => XDialogError::SystemError(s.clone()),
        }
    }
}

/// State shared between the handler (any thread) and the UI thread.
struct Shared {
    /// Set once the event loop exists (the UI thread publishes it after `build()`).
    proxy: OnceLock<EventLoopProxy<UserEvent>>,
    /// A `Wake` is in flight; cleared by the loop before it drains the inbox.
    wake_pending: Arc<AtomicBool>,
    /// Set when the UI thread can no longer serve requests.
    dead: OnceLock<Dead>,
    /// The request receiver: taken by the UI thread at start, put back here when the thread dies
    /// before the loop owns it (build failure) so queued requests can be answered.
    rx: Mutex<Option<Receiver<DialogMessageRequest>>>,
}

impl Shared {
    /// Answer every request still queued in the parked receiver with the dead error.
    fn drain_dead(&self) {
        let Some(dead) = self.dead.get() else { return };
        let guard = self.rx.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(rx) = guard.as_ref() {
            while let Ok(msg) = rx.try_recv() {
                answer_with_error(msg, &|| dead.error());
            }
        }
    }

    /// Switch to the dead state (first reason wins), park `rx` if the thread still has it, and
    /// answer everything queued.
    fn die(&self, reason: Dead, rx: Option<Receiver<DialogMessageRequest>>) {
        let _ = self.dead.set(reason);
        if let Some(rx) = rx {
            *self.rx.lock().unwrap_or_else(|e| e.into_inner()) = Some(rx);
        }
        self.drain_dead();
    }
}

/// The linux-direct request handler (installed by [`init_linux_direct`]).
struct DirectHandler {
    tx: Sender<DialogMessageRequest>,
    started: Once,
    shared: Arc<Shared>,
    xtheme: XDialogTheme,
}

impl DirectHandler {
    fn new(xtheme: XDialogTheme) -> Self {
        let (tx, rx) = channel();
        let shared = Arc::new(Shared { proxy: OnceLock::new(),
                                       wake_pending: Arc::new(AtomicBool::new(false)),
                                       dead: OnceLock::new(),
                                       rx: Mutex::new(Some(rx)) });
        DirectHandler { tx, started: Once::new(), shared, xtheme }
    }

    /// Spawn the UI thread (first request only).
    fn start(&self) {
        let shared = self.shared.clone();
        let xtheme = self.xtheme.clone();
        let spawned = std::thread::Builder::new().name("xdialog-ui".into()).spawn(move || ui_thread(shared, xtheme));
        if let Err(e) = spawned {
            error!("xdialog: could not start the linux-direct UI thread: {e}");
            self.shared.die(Dead::Error(format!("xdialog linux-direct: could not start the UI thread: {e}")), None);
        }
    }
}

impl DialogRequestHandler for DirectHandler {
    fn send(&self, message: DialogMessageRequest) -> Result<(), XDialogError> {
        if matches!(message, DialogMessageRequest::ExitEventLoop | DialogMessageRequest::None) {
            // The UI thread lives for the rest of the process; there is nothing to stop (and no
            // reason to start the thread, which takes the process's one winit loop).
            return Ok(());
        }
        if let Some(dead) = self.shared.dead.get() {
            answer_with_error(message, &|| dead.error());
            return Ok(());
        }
        // Requests buffer in the channel until the loop exists.
        if let Err(e) = self.tx.send(message) {
            // The loop dropped the receiver (it died): answer here.
            let dead = self.shared.dead.get().cloned().unwrap_or(Dead::Error("xdialog linux-direct: the UI thread is gone".into()));
            answer_with_error(e.0, &|| dead.error());
            return Ok(());
        }
        self.started.call_once(|| self.start());
        if self.shared.dead.get().is_some() {
            // Died between the check above and the send: make sure our request is answered.
            self.shared.drain_dead();
            return Ok(());
        }
        if let Some(proxy) = self.shared.proxy.get() {
            if !self.shared.wake_pending.swap(true, Ordering::SeqCst) && proxy.send_event(UserEvent::Wake).is_err() {
                // The loop is gone; `ui_thread` marks the handler dead right after `run_app`.
                self.shared.wake_pending.store(false, Ordering::SeqCst);
            }
        }
        Ok(())
    }
}

/// The body of the `xdialog-ui` thread. Never returns while the loop is healthy.
fn ui_thread(shared: Arc<Shared>, xtheme: XDialogTheme) {
    #[cfg(windows)]
    let _dpi = super::platform_win::ThreadDpiGuard::per_monitor_v2();

    let rx = shared.rx.lock().unwrap_or_else(|e| e.into_inner()).take();
    let Some(rx) = rx else {
        error!("xdialog linux-direct: the request receiver is missing");
        return;
    };

    let event_loop = match build_event_loop() {
        Ok(el) => el,
        Err(LoopBuildError::Recreation) => {
            error!("xdialog linux-direct: this process already created a winit 0.30 event loop");
            shared.die(Dead::Error(RECREATION_MSG.into()), Some(rx));
            return;
        }
        Err(LoopBuildError::Other(e)) => {
            warn!("xdialog linux-direct: {e}");
            shared.die(Dead::NoBackend, Some(rx));
            return;
        }
    };

    let _ui_thread = UiThreadMark::set();
    let proxy = event_loop.create_proxy();
    let _ = shared.proxy.set(proxy.clone());
    let inbox = Inbox { rx, wake_pending: shared.wake_pending.clone() };
    let mut app = OwnLoopApp::new(crate::backends::linux_egui::LinuxTheme::new(), xtheme, proxy, Some(inbox), false);

    let reason = match catch_unwind(AssertUnwindSafe(|| event_loop.run_app(&mut app))) {
        Ok(Ok(())) => "xdialog linux-direct: the UI event loop exited".to_string(),
        Ok(Err(e)) => format!("xdialog linux-direct: the UI event loop failed: {e}"),
        Err(_) => "xdialog UI thread panicked".to_string(),
    };
    error!("{reason}");
    // Mark dead BEFORE dropping the receiver (inside `app`): later requests are answered by the
    // handler; requests already queued are dropped with the receiver, so their callers get
    // `NoResult`, and open dialogs send `WindowClosed` as they are dropped. Never hangs.
    shared.die(Dead::Error(reason), None);
    if catch_unwind(AssertUnwindSafe(move || drop(app))).is_err() {
        error!("xdialog linux-direct: dropping the dialogs after the loop ended panicked");
    }
}

/// Initialize xdialog to show dialogs with the built-in Linux-style (egui) backend, without an
/// [`XDialogBuilder`](crate::XDialogBuilder) or an event loop of your own. Call once, before any
/// dialog function; later calls are ignored with a warning. This call is cheap: it only installs
/// a request handler.
///
/// On the first dialog request, xdialog starts a dedicated UI thread that lives for the rest of
/// the process and owns a winit 0.30 event loop. winit allows one event loop per process, so from
/// the first dialog onward this process cannot create another winit 0.30 loop (an application
/// that builds its own winit 0.30 loop must do so before the first dialog, or use the
/// `winit-host` feature instead). Dialog functions can be called from any thread, including
/// `main`.
///
/// # Errors
/// Errors are reported by the dialog functions, not here:
/// - [`XDialogError::NoBackendAvailable`](crate::XDialogError::NoBackendAvailable) when there is
///   no display server (X11/Wayland); the application keeps running.
/// - [`XDialogError::SystemError`](crate::XDialogError::SystemError) when this process already
///   created a winit 0.30 event loop (winit allows only one per process). If your application has
///   its own winit loop, enable the `winit-host` feature and integrate with it instead.
/// - [`XDialogError::SystemError`](crate::XDialogError::SystemError) for every call after the UI
///   thread failed unexpectedly (so later calls fail fast instead of hanging).
///
/// # Threads
/// Blocking calls (`show_message*`) made *on the xdialog UI thread*, i.e. from inside a progress
/// button callback, return
/// [`XDialogError::BlockingCallOnUiThread`](crate::XDialogError::BlockingCallOnUiThread) instead
/// of deadlocking. `show_progress*` there returns immediately; the window is created on the next
/// loop iteration. Only calls made directly on the UI thread are detected: a callback that waits
/// on another thread which is itself inside `show_message` still deadlocks.
///
/// Available on Linux and Windows (Windows is intended for development and testing of the Linux
/// look).
pub fn init_linux_direct(theme: XDialogTheme) {
    if !crate::channel::init_handler(Box::new(DirectHandler::new(theme))) {
        warn!("xdialog: init_linux_direct: a dialog backend is already initialized; ignoring");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{XDialogIcon, XDialogOptions};

    fn show(h: &DirectHandler, id: usize) -> oneshot::Receiver<Result<oneshot::Receiver<crate::model::XDialogResult>, XDialogError>> {
        let (tx, rx) = oneshot::channel();
        let opts = XDialogOptions { title: String::new(), main_instruction: String::new(), message: String::new(), icon: XDialogIcon::None, buttons: vec![] };
        h.send(DialogMessageRequest::ShowMessageWindow(id, opts, tx)).unwrap();
        rx
    }

    /// A dead handler answers queued and later requests at once (no UI thread involved).
    #[test]
    fn dead_handler_fails_fast() {
        let h = DirectHandler::new(XDialogTheme::Light);
        // Queue one request before the thread would take the receiver (bypass `start`).
        h.started.call_once(|| {});
        let queued = show(&h, 1);
        let rx = h.shared.rx.lock().unwrap().take();
        h.shared.die(Dead::Error("boom".into()), rx);
        assert!(matches!(queued.recv().unwrap(), Err(XDialogError::SystemError(s)) if s == "boom"));
        let later = show(&h, 2);
        assert!(matches!(later.recv().unwrap(), Err(XDialogError::SystemError(s)) if s == "boom"));
        // Updates are accepted and ignored.
        h.send(DialogMessageRequest::SetProgressValue(2, 0.5)).unwrap();
    }

    /// After the loop died and dropped its receiver, requests are still answered.
    #[test]
    fn dropped_receiver_fails_fast() {
        let h = DirectHandler::new(XDialogTheme::Light);
        h.started.call_once(|| {});
        drop(h.shared.rx.lock().unwrap().take());
        let r = show(&h, 1);
        assert!(matches!(r.recv().unwrap(), Err(XDialogError::SystemError(_))));
        h.shared.die(Dead::NoBackend, None);
        let r = show(&h, 2);
        assert!(matches!(r.recv().unwrap(), Err(XDialogError::NoBackendAvailable)));
    }
}
