mod gtk_dialog;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use crate::backends::XDialogBackendImpl;
use crate::model::*;

pub struct GtkBackend;

impl XDialogBackendImpl for GtkBackend {
    fn run_loop(receiver: Receiver<DialogMessageRequest>, _theme: XDialogTheme) {
        // GTK3 uses the system's native theme, so _theme is intentionally unused.
        if gtk::init().is_err() {
            error!("xdialog: Failed to initialize GTK3, falling back to FLTK");
            super::fltk::FltkBackend::run_loop(receiver, _theme);
            return;
        }

        let dialogs: Rc<RefCell<HashMap<usize, gtk_dialog::GtkDialog>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let dialogs_ref = dialogs.clone();

        // Poll the receiver channel from within the GTK main loop
        glib::timeout_add_local(Duration::from_millis(50), move || {
            loop {
                let message = match receiver.try_recv() {
                    Ok(msg) => msg,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        let mut d = dialogs_ref.borrow_mut();
                        for (_, dialog) in d.drain() {
                            dialog.destroy();
                        }
                        gtk::main_quit();
                        return glib::ControlFlow::Break;
                    }
                };

                match message {
                    DialogMessageRequest::None => {}
                    DialogMessageRequest::ExitEventLoop => {
                        let mut d = dialogs_ref.borrow_mut();
                        for (_, dialog) in d.drain() {
                            dialog.destroy();
                        }
                        gtk::main_quit();
                        return glib::ControlFlow::Break;
                    }
                    DialogMessageRequest::ShowMessageWindow(id, options, mut result) => {
                        let dialog = gtk_dialog::GtkDialog::new(id, options, false);
                        dialogs_ref.borrow_mut().insert(id, dialog);
                        result.send_ok();
                    }
                    DialogMessageRequest::ShowProgressWindow(id, options, mut result) => {
                        let dialog = gtk_dialog::GtkDialog::new(id, options, true);
                        dialogs_ref.borrow_mut().insert(id, dialog);
                        result.send_ok();
                    }
                    DialogMessageRequest::CloseWindow(id) => {
                        if let Some(dialog) = dialogs_ref.borrow_mut().remove(&id) {
                            dialog.close(id);
                        }
                    }
                    DialogMessageRequest::SetProgressIndeterminate(id) => {
                        if let Some(dialog) = dialogs_ref.borrow_mut().get_mut(&id) {
                            dialog.set_progress_indeterminate();
                        }
                    }
                    DialogMessageRequest::SetProgressValue(id, value) => {
                        if let Some(dialog) = dialogs_ref.borrow_mut().get_mut(&id) {
                            dialog.set_progress_value(value);
                        }
                    }
                    DialogMessageRequest::SetProgressText(id, text) => {
                        if let Some(dialog) = dialogs_ref.borrow_mut().get_mut(&id) {
                            dialog.set_progress_text(&text);
                        }
                    }
                }
            }

            // Pulse any indeterminate progress bars
            let d = dialogs_ref.borrow();
            for (_, dialog) in d.iter() {
                dialog.pulse_if_indeterminate();
            }

            glib::ControlFlow::Continue
        });

        gtk::main();
    }
}
