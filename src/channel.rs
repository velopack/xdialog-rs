use std::sync::mpsc::Sender;
use std::sync::OnceLock;

use crate::*;

static REQUEST_SEND: OnceLock<Sender<DialogMessageRequest>> = OnceLock::new();

pub fn init_sender(sender: Sender<DialogMessageRequest>) {
    if REQUEST_SEND.set(sender).is_err() {
        warn!("xdialog: init_sender called more than once, ignoring");
    }
}

pub fn send_request(message: DialogMessageRequest) -> Result<(), XDialogError> {
    match REQUEST_SEND.get() {
        Some(sender) => sender.send(message).map_err(XDialogError::SendFailed),
        None => Err(XDialogError::NotInitialized),
    }
}
