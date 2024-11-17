use crate::model::{DialogMessageRequest, XDialogTheme};
use std::sync::mpsc::Receiver;

pub mod fltk;
pub mod xaml_island;

pub trait XDialogBackendImpl<T> where T: Send + 'static {
    fn run(
        main: fn() -> T,
        receiver: Receiver<DialogMessageRequest>,
        xdialog_theme: XDialogTheme,
    ) -> T;
}

pub trait Tick {
    fn tick(&mut self, elapsed_secs: f32);
}
