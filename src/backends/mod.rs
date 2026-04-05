use crate::model::{DialogMessageRequest, XDialogTheme};
use std::sync::mpsc::Receiver;

#[cfg(target_os = "linux")]
pub mod fltk;

#[cfg(target_os = "linux")]
pub mod gtk3;

#[cfg(windows)]
pub mod win32;

#[cfg(target_os = "macos")]
pub mod appkit;

#[allow(unused)]
pub trait XDialogBackendImpl {
    fn run_loop(receiver: Receiver<DialogMessageRequest>, xdialog_theme: XDialogTheme);
}
