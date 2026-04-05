use std::sync::LazyLock;

use crate::backends::win32::taskdialog::TaskDialogManager;
use crate::channel::DialogRequestHandler;
use crate::*;

static MANAGER: LazyLock<TaskDialogManager> = LazyLock::new(TaskDialogManager::new);

struct Win32DirectHandler;

impl DialogRequestHandler for Win32DirectHandler {
    fn send(&self, message: DialogMessageRequest) -> Result<(), XDialogError> {
        match message {
            DialogMessageRequest::ShowMessageWindow(id, options, result) => {
                MANAGER.show(id, options, false, result);
                Ok(())
            }
            DialogMessageRequest::ShowProgressWindow(id, options, result) => {
                MANAGER.show(id, options, true, result);
                Ok(())
            }
            DialogMessageRequest::SetProgressValue(id, value) => {
                MANAGER.set_progress_value(id, value);
                Ok(())
            }
            DialogMessageRequest::SetProgressText(id, text) => {
                MANAGER.set_progress_text(id, &text);
                Ok(())
            }
            DialogMessageRequest::SetProgressIndeterminate(id) => {
                MANAGER.set_progress_indeterminate(id);
                Ok(())
            }
            DialogMessageRequest::CloseWindow(id) => {
                MANAGER.close(id);
                Ok(())
            }
            DialogMessageRequest::ExitEventLoop | DialogMessageRequest::None => Ok(()),
        }
    }
}

/// Initialize xdialog to use Win32 TaskDialog directly, without an event loop or
/// [`XDialogBuilder`]. This must be called before any dialog functions.
/// Can only be called once; subsequent calls will be ignored with a warning.
pub fn init_win32_direct() {
    crate::channel::init_handler(Box::new(Win32DirectHandler));
}
