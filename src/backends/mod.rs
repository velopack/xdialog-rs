use std::sync::mpsc::Receiver;
use crate::model::{DialogMessageRequest, XDialogTheme};

pub mod fltk;
mod fltk_fonts;
mod fltk_theme;

pub trait XDialogBackendImpl {
    fn run(main: fn() -> u16, receiver: Receiver<DialogMessageRequest>, xdialog_theme: XDialogTheme) -> u16;
}