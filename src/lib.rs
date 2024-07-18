#[macro_use]
extern crate log;

use std::sync::mpsc::channel;
use errors::*;
use model::*;
use state::*;

pub mod errors;
pub mod model;
mod backends;
mod state;
mod images;

pub fn run(main: fn() -> u16)
{
    let (send_message, receive_message) = channel::<DialogMessageRequest>();
    init_sender(send_message);
    backends::fltk::run_fltk_backend(main, receive_message);
}

pub fn set_silent_mode(silent: bool) {
    set_silent(silent);
}

pub fn show_message_box_info_ok_cancel<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(title: P1, main_instruction: P2, message: P3) -> Result<bool, XDialogError> {
    let data = MessageBoxData {
        title: title.as_ref().to_string(),
        main_instruction: main_instruction.as_ref().to_string(),
        message: message.as_ref().to_string(),
        icon: MessageBoxIcon::Information,
        buttons: vec!["Cancel".to_string(), "OK".to_string()],
    };
    let result = show_message_box(data)?;
    Ok(result == MessageBoxResult::ButtonPressed(1))
}

pub fn show_message_box(info: MessageBoxData) -> Result<MessageBoxResult, XDialogError> {
    if get_silent() {
        return Ok(MessageBoxResult::SilentMode);
    }

    let id = get_next_id();
    send_request(DialogMessageRequest::ShowMessageBox(id, info))?;
    loop {
        if let Some(result) = get_result(id) {
            return Ok(result);
        }
    }
}

