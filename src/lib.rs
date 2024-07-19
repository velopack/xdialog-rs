#[macro_use]
extern crate log;

use std::sync::mpsc::channel;

use errors::*;
pub use model::*;
use state::*;

use crate::backends::XDialogBackendImpl;

pub mod errors;
mod model;
mod backends;
mod state;
mod images;

#[derive(Debug)]
pub struct XDialogBuilder
{
    backend: XDialogBackend,
    theme: XDialogTheme,
}

impl Default for XDialogBuilder
{
    fn default() -> XDialogBuilder
    {
        XDialogBuilder {
            backend: XDialogBackend::Automatic,
            theme: XDialogTheme::SystemDefault,
        }
    }
}

impl XDialogBuilder
{
    pub fn new() -> XDialogBuilder
    {
        XDialogBuilder::default()
    }

    pub fn with_backend(mut self, backend: XDialogBackend) -> XDialogBuilder
    {
        self.backend = backend;
        self
    }

    pub fn with_theme(mut self, theme: XDialogTheme) -> XDialogBuilder
    {
        self.theme = theme;
        self
    }

    pub fn run(self, main: fn() -> u16) -> u16
    {
        let (send_message, receive_message) = channel::<DialogMessageRequest>();
        init_sender(send_message);

        match self.backend {
            XDialogBackend::Automatic => {
                backends::fltk::FltkBackend::run(main, receive_message, self.theme)
            }
            XDialogBackend::Fltk => {
                backends::fltk::FltkBackend::run(main, receive_message, self.theme)
            }
        }
    }
}

pub fn set_silent_mode(silent: bool) {
    set_silent(silent);
}

pub fn show_message_box_info_ok_cancel<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(title: P1, main_instruction: P2, message: P3) -> Result<bool, XDialogError> {
    let data = XDialogMessageBox {
        title: title.as_ref().to_string(),
        main_instruction: main_instruction.as_ref().to_string(),
        message: message.as_ref().to_string(),
        icon: XDialogIcon::Information,
        buttons: vec!["Cancel".to_string(), "OK".to_string()],
    };
    let result = show_message_box(data)?;
    Ok(result == XDialogResult::ButtonPressed(1))
}

pub fn show_message_box(info: XDialogMessageBox) -> Result<XDialogResult, XDialogError> {
    if get_silent() {
        return Ok(XDialogResult::SilentMode);
    }

    let id = get_next_id();
    send_request(DialogMessageRequest::ShowMessageBox(id, info))?;
    loop {
        if let Some(result) = get_result(id) {
            return Ok(result);
        }
    }
}

