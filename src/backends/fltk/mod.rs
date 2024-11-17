mod fltk_button;
mod fltk_dialog;
mod fltk_fonts;
mod fltk_progress;
mod fltk_theme;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Instant;

use fltk::app;

use crate::backends::Tick;
use crate::model::*;

use super::{fltk::fltk_dialog::CustomFltkDialog, XDialogBackendImpl};

pub struct FltkBackend;

impl XDialogBackendImpl for FltkBackend {
    fn run(
        main: fn() -> i32,
        receiver: Receiver<DialogMessageRequest>,
        theme: XDialogTheme,
    ) -> i32 {
        let app_instance = app::App::default();

        let spacing = super::fltk::fltk_theme::apply_theme(&app_instance, theme);

        let t = thread::spawn(move || {
            return main();
        });

        let dialogs1: Rc<RefCell<HashMap<usize, CustomFltkDialog>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let dialogs2 = dialogs1.clone();
        let current_time = Rc::new(RefCell::new(Instant::now()));

        // let mut tick_mgr = Rc::new(RefCell::new(TickManager::new()));
        // let mut tick_mgr2 = tick_mgr.clone();

        app::add_timeout3(0.008, move |handle| {
            let mut t = dialogs1.borrow_mut();
            let mut current_time = current_time.borrow_mut();
            let now = Instant::now();
            let elapsed = now.duration_since(*current_time).as_secs_f32();
            *current_time = now;
            for (_, dialog) in t.iter_mut() {
                dialog.tick(elapsed);
            }
            app::repeat_timeout3(0.008, handle);
        });

        loop {
            if let Err(e) = app::wait_for(0.1) {
                error!("xdialog event loop fatal error: {:?}", e);
                return t.join().unwrap();
            }

            if t.is_finished() {
                app::quit();
                return t.join().unwrap();
            }

            // TODO: clean up finished message box windows with window::Window::delete(hWnd);

            loop {
                // read all messages until there are no more queued
                let message = receiver.try_recv().unwrap_or(DialogMessageRequest::None);
                if message == DialogMessageRequest::None {
                    break;
                }

                match message {
                    DialogMessageRequest::None => {}
                    DialogMessageRequest::ShowMessageWindow(id, data) => {
                        let mut d = CustomFltkDialog::new(id, data, &spacing, false);
                        d.show();
                        dialogs2.borrow_mut().insert(id, d);
                    }
                    DialogMessageRequest::ExitEventLoop => {
                        app::quit();
                        break;
                    }
                    DialogMessageRequest::CloseWindow(id) => {
                        if let Some(dialog) = dialogs2.borrow_mut().get_mut(&id) {
                            dialog.close();
                        }
                    }
                    DialogMessageRequest::ShowProgressWindow(id, data) => {
                        let mut d = CustomFltkDialog::new(id, data, &spacing, true);
                        d.show();
                        dialogs2.borrow_mut().insert(id, d);
                    }
                    DialogMessageRequest::SetProgressIndeterminate(id) => {
                        if let Some(dialog) = dialogs2.borrow_mut().get_mut(&id) {
                            dialog.set_progress_indeterminate();
                        }
                    }
                    DialogMessageRequest::SetProgressValue(id, value) => {
                        if let Some(dialog) = dialogs2.borrow_mut().get_mut(&id) {
                            dialog.set_progress_value(value);
                        }
                    }
                    DialogMessageRequest::SetProgressText(id, text) => {
                        if let Some(dialog) = dialogs2.borrow_mut().get_mut(&id) {
                            dialog.set_body_text(&text);
                        }
                    }
                }
            }
        }
    }
}
