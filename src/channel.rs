use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, SendError, Sender};
use std::sync::{Arc, Mutex, OnceLock};

use crate::*;

/// Dispatches dialog requests to a backend.
pub(crate) trait DialogRequestHandler: Send + Sync {
    fn send(&self, message: DialogMessageRequest) -> Result<(), XDialogError>;

    /// Make the backend's event loop iterate, if it has one.
    fn wake(&self) {}
}

static REQUEST_HANDLER: OnceLock<Box<dyn DialogRequestHandler>> = OnceLock::new();

/// Install the process-wide request handler; `false` (and a warning) if one already exists.
pub(crate) fn init_handler(handler: Box<dyn DialogRequestHandler>) -> bool {
    if REQUEST_HANDLER.set(handler).is_err() {
        warn!("xdialog: init_handler called more than once, ignoring");
        return false;
    }
    true
}

pub(crate) fn handler_installed() -> bool {
    REQUEST_HANDLER.get().is_some()
}

pub(crate) fn send_request(message: DialogMessageRequest) -> Result<(), XDialogError> {
    match REQUEST_HANDLER.get() {
        Some(handler) => handler.send(message),
        None => Err(XDialogError::NotInitialized),
    }
}

/// Make xdialog's event loop (builder or host) iterate, if there is one.
pub(crate) fn wake_ui() {
    if let Some(handler) = REQUEST_HANDLER.get() {
        handler.wake();
    }
}

/// A wake-up callback: must make the event loop iterate, must not block.
pub(crate) type WakeFn = Box<dyn Fn() + Send>;

/// Request queue + coalesced wake, shared by the installed handler ([`InboxHandler`]) and the
/// backend that drains it. The drain clears `wake_pending` BEFORE reading the channel
/// ([`Inbox::begin_drain`]), so a request sent during or after a drain wakes the loop again.
pub(crate) struct Inbox {
    tx: Sender<DialogMessageRequest>,
    wake_pending: AtomicBool,
    /// Nobody drains any more ([`Inbox::close`]): wakes are no-ops.
    closed: AtomicBool,
    /// `Mutex`: wakers need only be `Send` (a Windows `EventLoopProxy` isn't `Sync`).
    waker: Mutex<WakeFn>,
}

impl Inbox {
    pub(crate) fn new(waker: WakeFn) -> (Arc<Inbox>, Receiver<DialogMessageRequest>) {
        let (tx, rx) = channel();
        (Arc::new(Inbox { tx, wake_pending: AtomicBool::new(false), closed: AtomicBool::new(false), waker: Mutex::new(waker) }), rx)
    }

    /// An inbox nobody serves (no backend can run): every request is answered with
    /// `NoBackendAvailable` (see [`InboxHandler`]).
    pub(crate) fn closed() -> Arc<Inbox> {
        Inbox::new(Box::new(|| {})).0
    }

    /// Call the waker unless a wake is already outstanding or the inbox is closed.
    pub(crate) fn wake(&self) {
        if !self.closed.load(Ordering::SeqCst) && !self.wake_pending.swap(true, Ordering::SeqCst) {
            (self.waker.lock().unwrap_or_else(|e| e.into_inner()))();
        }
    }

    /// The backend stopped draining for good (rejected requests still notify their callers, which
    /// must not wake a loop that no longer serves dialogs).
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    /// Replace the waker (macOS builder mode: the event loop is built when the first dialog is
    /// requested, so it gets its waker then).
    #[cfg(target_os = "macos")]
    pub(crate) fn set_waker(&self, waker: WakeFn) {
        *self.waker.lock().unwrap_or_else(|e| e.into_inner()) = waker;
    }

    /// Called by the backend right before it drains the receiver.
    pub(crate) fn begin_drain(&self) {
        self.wake_pending.store(false, Ordering::SeqCst);
    }
}

/// The request handler of host mode and of builder mode (drawn and AppKit backends, or none);
/// Win32 TaskDialog installs `TaskDialogManager` instead. Once the receiver is gone (host shut
/// down, builder loop ended) requests get `NoBackendAvailable`.
pub(crate) struct InboxHandler(pub Arc<Inbox>);

impl DialogRequestHandler for InboxHandler {
    fn send(&self, message: DialogMessageRequest) -> Result<(), XDialogError> {
        match self.0.tx.send(message) {
            Ok(()) => self.0.wake(),
            Err(SendError(message)) => reject(message),
        }
        Ok(())
    }

    fn wake(&self) {
        self.0.wake();
    }
}

/// Answer a creation request with `NoBackendAvailable`; other requests are ignored.
pub(crate) fn reject(message: DialogMessageRequest) {
    if let DialogMessageRequest::ShowMessageWindow(_, _, reply) | DialogMessageRequest::ShowProgressWindow(_, _, reply, _) = message {
        reply.failed(XDialogError::NoBackendAvailable);
    }
}

// ---- UI-thread marker (blocking-call detection) ----
//
// Set on the thread that runs xdialog's drawn-dialog event loop (builder mode), the thread that created an
// `XDialogHost` (until it is shut down), and the AppKit loop thread (their button callbacks run there). Never set by
// win32 / win32-direct / maccf-direct, whose dialogs run on their own threads.

thread_local! {
    static UI_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// Marks the current thread as an xdialog UI thread and unmarks it when dropped (also when the
/// loop unwinds: builder mode runs on the user's main thread).
pub(crate) struct UiThreadMark;

impl UiThreadMark {
    pub(crate) fn set() -> Self {
        UI_THREAD.with(|c| c.set(true));
        UiThreadMark
    }
}

impl Drop for UiThreadMark {
    fn drop(&mut self) {
        UI_THREAD.with(|c| c.set(false));
    }
}

/// Whether the current thread is an xdialog UI thread (see [`UiThreadMark`]).
pub(crate) fn is_ui_thread() -> bool {
    UI_THREAD.with(|c| c.get())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn wake_coalesces_until_drain_and_send_after_drop_fails_fast() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let (inbox, rx) = Inbox::new(Box::new(move || {
                                          c.fetch_add(1, Ordering::SeqCst);
                                      }));
        let handler = InboxHandler(inbox.clone());
        for _ in 0..5 {
            handler.send(DialogMessageRequest::None).unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        inbox.begin_drain();
        assert_eq!(rx.try_iter().count(), 5);
        handler.send(DialogMessageRequest::CloseWindow(1)).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2, "a request after the drain wakes again");

        drop(rx);
        let (tx, crx) = crate::oneshot::channel();
        let opts = XDialogOptions::default();
        handler.send(DialogMessageRequest::ShowMessageWindow(1, opts, DialogReply::Message(tx))).unwrap();
        assert!(matches!(crx.try_recv(), Ok(Err(XDialogError::NoBackendAvailable))));

        inbox.begin_drain();
        inbox.close();
        inbox.wake();
        assert_eq!(calls.load(Ordering::SeqCst), 2, "a closed inbox never wakes");
    }
}
