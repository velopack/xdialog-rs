use crate::model::{DialogMessageRequest, XDialogTheme};
use std::sync::mpsc::Receiver;

#[cfg(feature = "fltk")]
pub mod fltk;

#[cfg(all(target_os = "linux", feature = "gtk3"))]
pub mod gtk3;

#[cfg(windows)]
pub mod win32;

#[allow(unused)]
pub trait XDialogBackendImpl {
    fn run_loop(receiver: Receiver<DialogMessageRequest>, xdialog_theme: XDialogTheme);
}
