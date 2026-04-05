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
        // GTK4 uses the system's native theme, so _theme is intentionally unused.
        if gtk4::init().is_err() {
            error!("xdialog: Failed to initialize GTK4, falling back to FLTK backend");
            super::fltk::FltkBackend::run_loop(receiver, _theme);
            return;
        }

        let main_loop = gtk4::glib::MainLoop::new(None, false);
        let main_loop_quit = main_loop.clone();

        let dialogs: Rc<RefCell<HashMap<usize, gtk_dialog::GtkDialog>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let dialogs_ref = dialogs.clone();

        // Poll the receiver channel from within the GTK main loop
        gtk4::glib::timeout_add_local(Duration::from_millis(50), move || {
            loop {
                let message = match receiver.try_recv() {
                    Ok(msg) => msg,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        let mut d = dialogs_ref.borrow_mut();
                        for (_, dialog) in d.drain() {
                            dialog.destroy();
                        }
                        main_loop_quit.quit();
                        return gtk4::glib::ControlFlow::Break;
                    }
                };

                match message {
                    DialogMessageRequest::None => {}
                    DialogMessageRequest::ExitEventLoop => {
                        let mut d = dialogs_ref.borrow_mut();
                        for (_, dialog) in d.drain() {
                            dialog.destroy();
                        }
                        main_loop_quit.quit();
                        return gtk4::glib::ControlFlow::Break;
                    }
                    DialogMessageRequest::ShowMessageWindow(id, options, creation) => {
                        let (dialog_sender, dialog_receiver) = oneshot::channel();
                        let dialog = gtk_dialog::GtkDialog::new(options, false, dialog_sender);
                        dialogs_ref.borrow_mut().insert(id, dialog);
                        let _ = creation.send(Ok(dialog_receiver));
                    }
                    DialogMessageRequest::ShowProgressWindow(id, options, creation) => {
                        let (dialog_sender, dialog_receiver) = oneshot::channel();
                        let dialog = gtk_dialog::GtkDialog::new(options, true, dialog_sender);
                        dialogs_ref.borrow_mut().insert(id, dialog);
                        let _ = creation.send(Ok(dialog_receiver));
                    }
                    DialogMessageRequest::CloseWindow(id) => {
                        if let Some(dialog) = dialogs_ref.borrow_mut().remove(&id) {
                            dialog.close();
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

            gtk4::glib::ControlFlow::Continue
        });

        main_loop.run();
    }
}
