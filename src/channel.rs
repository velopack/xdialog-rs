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

pub fn init_handler(handler: Box<dyn DialogRequestHandler>) {
    if REQUEST_HANDLER.set(handler).is_err() {
        warn!("xdialog: init_handler called more than once, ignoring");
    }
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
