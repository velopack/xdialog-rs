#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;

use std::sync::mpsc::channel;

pub use message::*;
pub use model::*;
pub use progress::*;
use state::*;

use crate::backends::XDialogBackendImpl;

pub mod errors;
mod model;
mod backends;
mod state;
mod images;
mod progress;
mod message;

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

    pub fn run(self, main: fn() -> i32) -> i32
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

