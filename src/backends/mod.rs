use crate::model::{DialogMessageRequest, XDialogTheme};
use std::sync::mpsc::Receiver;

// Cross-platform pure-Rust software renderer (winit + softbuffer + tiny-skia).
// The backend on Linux; Windows and macOS use their native backends.
pub mod skia;

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
