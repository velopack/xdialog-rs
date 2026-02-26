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
pub trait DialogManager {
    fn show(&mut self, id: usize, options: XDialogOptions, has_progress: bool) -> Result<(), XDialogError>;
    fn close(&mut self, id: usize);
    fn close_all(&mut self);
    fn set_progress_value(&mut self, id: usize, progress: f32);
    fn set_progress_text(&mut self, id: usize, text: &str);
    fn set_progress_indeterminate(&mut self, id: usize);
}

pub trait Tick {
    fn tick(&mut self, elapsed_secs: f32);
}
