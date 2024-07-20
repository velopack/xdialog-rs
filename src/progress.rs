use crate::{DialogMessageRequest, XDialogIcon, XDialogOptions};
use crate::errors::XDialogError;
use crate::state::{get_next_id, send_request};

pub fn show_progress_dialog<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    icon: XDialogIcon, title: P1, main_instruction: P2, message: P3) -> Result<ProgressDialogProxy, XDialogError> {
    let id = get_next_id();

    let data = XDialogOptions {
        title: title.as_ref().to_string(),
        main_instruction: main_instruction.as_ref().to_string(),
        message: message.as_ref().to_string(),
        icon,
        buttons: vec![],
    };

    send_request(DialogMessageRequest::ShowProgressWindow(id, data))?;
    Ok(ProgressDialogProxy { id })
}

pub struct ProgressDialogProxy {
    id: usize,
}

impl ProgressDialogProxy {
    pub fn set_indeterminate(&self) -> Result<(), XDialogError> {
        send_request(DialogMessageRequest::SetProgressIndeterminate(self.id))
    }

    pub fn set_value(&self, value: f32) -> Result<(), XDialogError> {
        send_request(DialogMessageRequest::SetProgressValue(self.id, value))
    }

    pub fn set_text<P: AsRef<str>>(&self, text: P) -> Result<(), XDialogError> {
        send_request(DialogMessageRequest::SetProgressText(self.id, text.as_ref().to_string()))
    }

    pub fn close(&self) -> Result<(), XDialogError> {
        send_request(DialogMessageRequest::CloseWindow(self.id))
    }
}