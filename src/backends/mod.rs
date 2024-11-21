use winit::event_loop::ActiveEventLoop;

use crate::{
    model::{DialogMessageRequest, XDialogTheme},
    XDialogError, XDialogOptions, XDialogWebviewOptions, XDialogWindowState,
};
use std::sync::mpsc::Receiver;

pub mod fltk;
pub mod native;

pub trait XDialogBackendImpl {
    fn run_loop(receiver: Receiver<DialogMessageRequest>, xdialog_theme: XDialogTheme);
}

pub trait DialogManager {
    fn show(&mut self, id: usize, options: XDialogOptions, has_progress: bool) -> Result<(), XDialogError>;
    fn close(&mut self, id: usize);
    fn close_all(&mut self);
    fn set_progress_value(&mut self, id: usize, progress: f32);
    fn set_progress_text(&mut self, id: usize, text: &str);
    fn set_progress_indeterminate(&mut self, id: usize);
}

pub trait WebviewManager {
    fn show(&mut self, id: usize, options: XDialogWebviewOptions, event_loop: &ActiveEventLoop) -> Result<(), XDialogError>;
    fn set_title(&mut self, id: usize, title: &str) -> Result<(), XDialogError>;
    fn set_html(&mut self, id: usize, html: &str) -> Result<(), XDialogError>;
    fn set_position(&mut self, id: usize, x: i32, y: i32) -> Result<(), XDialogError>;
    fn set_size(&mut self, id: usize, width: i32, height: i32) -> Result<(), XDialogError>;
    fn set_zoom_level(&mut self, id: usize, zoom_level: f64) -> Result<(), XDialogError>;
    fn set_window_state(&mut self, id: usize, state: XDialogWindowState) -> Result<(), XDialogError>;
    fn eval_js(&mut self, id: usize, js: &str) -> Result<(), XDialogError>;
    fn close(&mut self, id: usize);
    fn close_all(&mut self);
}

pub trait Tick {
    fn tick(&mut self, elapsed_secs: f32);
}
