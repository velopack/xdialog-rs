#[cfg(windows)]
mod taskdialog;

use std::sync::mpsc::Receiver;

use crate::{
    backends::{DialogManager, XDialogBackendImpl},
    DialogMessageRequest, XDialogTheme,
};

pub struct Win32Backend;

impl XDialogBackendImpl for Win32Backend {
    fn run_loop(receiver: Receiver<DialogMessageRequest>, _theme: XDialogTheme) {
        let mut dialogs: Box<dyn DialogManager> =
            Box::new(taskdialog::TaskDialogManager::new());
        while let Ok(message) = receiver.recv() {
            match message {
                DialogMessageRequest::None => {}
                DialogMessageRequest::ShowMessageWindow(id, options, mut result) => {
                    result.send_result(dialogs.show(id, options, false));
                }
                DialogMessageRequest::ExitEventLoop => {
                    dialogs.close_all();
                    return;
                }
                DialogMessageRequest::CloseWindow(id) => {
                    dialogs.close(id);
                }
                DialogMessageRequest::ShowProgressWindow(id, options, mut result) => {
                    result.send_result(dialogs.show(id, options, true));
                }
                DialogMessageRequest::SetProgressIndeterminate(id) => {
                    dialogs.set_progress_indeterminate(id);
                }
                DialogMessageRequest::SetProgressValue(id, value) => {
                    dialogs.set_progress_value(id, value);
                }
                DialogMessageRequest::SetProgressText(id, text) => {
                    dialogs.set_progress_text(id, &text);
                }
            }
        }
        dialogs.close_all();
    }
}
