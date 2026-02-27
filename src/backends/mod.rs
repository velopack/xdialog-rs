use crate::model::{DialogMessageRequest, XDialogTheme};
use std::sync::mpsc::Receiver;

#[cfg(windows)]
use crate::{XDialogError, XDialogOptions};

pub mod fltk;
#[cfg(windows)]
pub mod win32;

pub trait XDialogBackendImpl {
    fn run_loop(receiver: Receiver<DialogMessageRequest>, xdialog_theme: XDialogTheme);
}

#[cfg(windows)]
/// Trait for managing dialog windows. Allows showing, closing, and updating progress dialogs.
pub trait DialogManager {
    /// Show a dialog with the given options. If `has_progress` is true, a progress bar is included.
    fn show(&mut self, id: usize, options: XDialogOptions, has_progress: bool) -> Result<(), XDialogError>;
    /// Close a specific dialog by ID.
    fn close(&mut self, id: usize);
    /// Close all open dialogs.
    fn close_all(&mut self);
    /// Set the progress bar value (0.0 to 1.0) for the given dialog.
    fn set_progress_value(&mut self, id: usize, progress: f32);
    /// Set the progress body text for the given dialog.
    fn set_progress_text(&mut self, id: usize, text: &str);
    /// Set the progress bar to indeterminate (marquee) mode.
    fn set_progress_indeterminate(&mut self, id: usize);
}

pub trait Tick {
    fn tick(&mut self, elapsed_secs: f32);
}
