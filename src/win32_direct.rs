use std::sync::LazyLock;

use crate::backends::win32::taskdialog::TaskDialogManager;
use crate::*;

static MANAGER: LazyLock<TaskDialogManager> = LazyLock::new(TaskDialogManager::new);

fn manager() -> &'static TaskDialogManager {
    &MANAGER
}

pub fn send_request(message: DialogMessageRequest) -> Result<(), XDialogError> {
    match message {
        DialogMessageRequest::ShowMessageWindow(id, options, result) => {
            manager().show(id, options, false, result);
            Ok(())
        }
        DialogMessageRequest::ShowProgressWindow(id, options, result) => {
            manager().show(id, options, true, result);
            Ok(())
        }
        DialogMessageRequest::SetProgressValue(id, value) => {
            manager().set_progress_value(id, value);
            Ok(())
        }
        DialogMessageRequest::SetProgressText(id, text) => {
            manager().set_progress_text(id, &text);
            Ok(())
        }
        DialogMessageRequest::SetProgressIndeterminate(id) => {
            manager().set_progress_indeterminate(id);
            Ok(())
        }
        DialogMessageRequest::CloseWindow(id) => {
            manager().close(id);
            Ok(())
        }
        DialogMessageRequest::ExitEventLoop | DialogMessageRequest::None => Ok(()),
    }
}
