use std::cell::Cell;
use std::sync::mpsc::Sender;
use std::sync::OnceLock;

use crate::*;

/// Trait for dispatching dialog requests to a backend.
/// Implementations must be thread-safe (`Send + Sync`).
pub trait DialogRequestHandler: Send + Sync {
    /// Send a dialog message request to the backend.
    fn send(&self, message: DialogMessageRequest) -> Result<(), XDialogError>;
}

static REQUEST_HANDLER: OnceLock<Box<dyn DialogRequestHandler>> = OnceLock::new();

/// Install the process-wide request handler. Returns `true` if `handler` was installed, `false`
/// if a handler already existed (the new one is dropped and a warning is logged).
pub fn init_handler(handler: Box<dyn DialogRequestHandler>) -> bool {
    if REQUEST_HANDLER.set(handler).is_err() {
        warn!("xdialog: init_handler called more than once, ignoring");
        return false;
    }
    true
}

pub struct ChannelHandler {
    pub sender: Sender<DialogMessageRequest>,
}

impl DialogRequestHandler for ChannelHandler {
    fn send(&self, message: DialogMessageRequest) -> Result<(), XDialogError> {
        self.sender.send(message).map_err(|e| XDialogError::SendFailed(e.to_string()))
    }
}

pub fn send_request(message: DialogMessageRequest) -> Result<(), XDialogError> {
    match REQUEST_HANDLER.get() {
        Some(handler) => handler.send(message),
        None => Err(XDialogError::NotInitialized),
    }
}

// ---- UI-thread marker (blocking-call detection) ----
//
// Set by the thread that runs an xdialog egui event loop: the builder loop thread (after the
// event loop was built), the linux-direct UI thread (after build), and the winit-host thread at
// its first `host::pump`; and by the AppKit loop thread (its button callbacks run there). Never
// set by win32 / win32-direct / maccf-direct, whose dialogs run on their own threads.

thread_local! {
    static UI_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// Mark (or unmark) the current thread as an xdialog UI thread.
#[allow(dead_code)]
pub fn mark_ui_thread(on: bool) {
    UI_THREAD.with(|c| c.set(on));
}

/// Whether the current thread is an xdialog UI thread (see [`mark_ui_thread`]).
#[allow(dead_code)]
pub fn is_ui_thread() -> bool {
    UI_THREAD.with(|c| c.get())
}

/// Returns `Err(XDialogError::BlockingCallOnUiThread)` when called on an xdialog UI thread, where
/// waiting for a dialog result would deadlock the loop that has to produce it. `op` names the
/// blocking operation for the log.
#[allow(dead_code)]
pub fn ui_thread_guard(op: &str) -> Result<(), XDialogError> {
    if is_ui_thread() {
        warn!("xdialog: {op} would block the xdialog UI thread and deadlock; call it from another thread");
        return Err(XDialogError::BlockingCallOnUiThread);
    }
    Ok(())
}
