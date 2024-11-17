use crate::model::{DialogMessageRequest, XDialogTheme};
use std::sync::mpsc::Receiver;

pub mod fltk;

pub trait XDialogBackendImpl {
    fn run(
        main: fn() -> i32,
        receiver: Receiver<DialogMessageRequest>,
        xdialog_theme: XDialogTheme,
    ) -> i32;
}

pub trait Tick {
    fn tick(&mut self, elapsed_secs: f32);
}
