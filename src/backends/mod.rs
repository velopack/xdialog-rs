use crate::model::{DialogMessageRequest, XDialogTheme};
use std::sync::mpsc::Receiver;

#[cfg(target_os = "linux")]
pub mod skia;

#[cfg(all(target_os = "linux", feature = "fltk"))]
pub mod fltk;

#[cfg(all(target_os = "linux", feature = "gtk"))]
pub mod gtk3;

#[cfg(windows)]
pub mod win32;

#[cfg(all(windows, feature = "win32-direct"))]
pub mod win32_direct;

#[cfg(target_os = "macos")]
pub mod appkit;

#[cfg(all(target_os = "macos", feature = "maccf-direct"))]
pub mod maccf_direct;

#[allow(unused)]
pub trait XDialogBackendImpl {
    fn run_loop(receiver: Receiver<DialogMessageRequest>, xdialog_theme: XDialogTheme);
}
