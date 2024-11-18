use crate::model::{DialogMessageRequest, XDialogTheme};
use std::sync::mpsc::Receiver;

pub mod fltk;
// pub mod xaml_island;
// pub mod webview;
// pub mod mshtml;

pub trait XDialogBackendImpl {
    fn run_loop(
        receiver: Receiver<DialogMessageRequest>,
        xdialog_theme: XDialogTheme,
    );
}

pub trait Tick {
    fn tick(&mut self, elapsed_secs: f32);
}
