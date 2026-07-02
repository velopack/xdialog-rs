mod appkit_dialog;

use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{LazyLock, Mutex};

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
use objc2::{msg_send, sel};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventMask};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode};

use crate::backends::XDialogBackendImpl;
use crate::model::*;
use crate::{ProgressButtonCallback, ProgressDialogProxy};

use appkit_dialog::AppKitDialog;

// Global map of dialog result senders, keyed by dialog id.
// Required because the button_clicked handler is an extern "C" callback
// that can't capture Rust state — it looks up the sender by dialog id
// extracted from the button's tag.
static RESULT_SENDERS: LazyLock<Mutex<HashMap<usize, oneshot::Sender<XDialogResult>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// Global map of progress-dialog button callbacks, keyed by dialog id. Same rationale as
// RESULT_SENDERS: the extern "C" click handler can't capture Rust state.
static PROGRESS_CALLBACKS: LazyLock<Mutex<HashMap<usize, ProgressButtonCallback>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn send_dialog_result(id: usize, result: XDialogResult) {
    if let Some(sender) = RESULT_SENDERS.lock().unwrap_or_else(|e| e.into_inner()).remove(&id) {
        let _ = sender.send(result);
    }
}

fn remove_progress_callback(id: usize) {
    PROGRESS_CALLBACKS.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
}

pub struct AppKitBackend;

fn register_button_handler_class() {
    if AnyClass::get(c"XDialogButtonClickHandler").is_some() {
        return;
    }

    let superclass = AnyClass::get(c"NSObject").unwrap();
    let mut builder = ClassBuilder::new(c"XDialogButtonClickHandler", superclass).unwrap();

    unsafe {
        builder.add_method(
            sel!(buttonClicked:),
            button_clicked as unsafe extern "C" fn(NonNull<AnyObject>, Sel, NonNull<AnyObject>),
        );
    }

    builder.register();
}

unsafe extern "C" fn button_clicked(
    _this: NonNull<AnyObject>,
    _cmd: Sel,
    sender: NonNull<AnyObject>,
) {
    let sender = unsafe { sender.as_ref() };
    let tag: isize = unsafe { msg_send![sender, tag] };
    let dialog_id = (tag >> 16) as usize;
    let button_index = (tag & 0xFFFF) as usize;

    // If a progress button callback is registered, it decides whether the dialog closes.
    // Otherwise fall back to the default behavior: deliver the result and close.
    let mut keep_open = false;
    let has_callback = {
        let mut callbacks = PROGRESS_CALLBACKS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cb) = callbacks.get_mut(&dialog_id) {
            let proxy = ProgressDialogProxy::non_owning(dialog_id);
            keep_open = (cb.0)(button_index, &proxy);
            true
        } else {
            false
        }
    };

    if !has_callback {
        send_dialog_result(dialog_id, XDialogResult::ButtonPressed(button_index));
    }

    if !keep_open {
        let window: Option<Retained<AnyObject>> = unsafe { msg_send![sender, window] };
        if let Some(window) = window {
            let () = unsafe { msg_send![&*window, orderOut: std::ptr::null::<AnyObject>()] };
        }
    }
}

fn create_handler_instance() -> Retained<AnyObject> {
    let cls = AnyClass::get(c"XDialogButtonClickHandler").unwrap();
    unsafe { msg_send![cls, new] }
}

impl XDialogBackendImpl for AppKitBackend {
    fn run_loop(receiver: Receiver<DialogMessageRequest>, _theme: XDialogTheme) {
        // Headless phase: do not touch AppKit until a dialog is actually requested.
        // Connecting to the window server registers the process with LaunchServices —
        // when the executable lives inside another app's bundle (e.g. an updater in
        // Contents/MacOS) it checks in as a second instance of that app, which can
        // surface in the Dock and steal focus. Most invocations of such tools never
        // show any UI, so stay completely invisible until one does.
        let first_message = loop {
            match receiver.recv() {
                Err(_) => return,
                Ok(DialogMessageRequest::ExitEventLoop) => return,
                Ok(
                    message @ (DialogMessageRequest::ShowMessageWindow(..)
                    | DialogMessageRequest::ShowProgressWindow(..)),
                ) => break message,
                // close/progress updates for dialogs that were never created are no-ops
                Ok(_) => continue,
            }
        };

        register_button_handler_class();
        let handler = create_handler_instance();

        let app = unsafe {
            let app = NSApplication::sharedApplication(objc2::MainThreadMarker::new_unchecked());
            // the equivalent of LSUIElement: no Dock icon or menu bar, but windows can
            // still be shown and focused. Must be set before the app finishes launching.
            app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            app.finishLaunching();
            app
        };

        let mut dialogs: HashMap<usize, AppKitDialog> = HashMap::new();

        if Self::handle_message(first_message, &mut dialogs, &handler) {
            return;
        }

        loop {
            // Pump AppKit events with 50ms timeout
            loop {
                let event: Option<Retained<NSEvent>> = unsafe {
                    app.nextEventMatchingMask_untilDate_inMode_dequeue(
                        NSEventMask::Any,
                        Some(&NSDate::dateWithTimeIntervalSinceNow(0.05)),
                        NSDefaultRunLoopMode,
                        true,
                    )
                };
                match event {
                    Some(event) => app.sendEvent(&event),
                    None => break,
                }
            }

            // Drain remaining events without waiting
            loop {
                let event: Option<Retained<NSEvent>> = unsafe {
                    app.nextEventMatchingMask_untilDate_inMode_dequeue(
                        NSEventMask::Any,
                        Some(&NSDate::distantPast()),
                        NSDefaultRunLoopMode,
                        true,
                    )
                };
                match event {
                    Some(event) => app.sendEvent(&event),
                    None => break,
                }
            }

            // Clean up closed windows
            dialogs.retain(|id, dialog| {
                if dialog.is_visible() {
                    true
                } else {
                    remove_progress_callback(*id);
                    send_dialog_result(*id, XDialogResult::WindowClosed);
                    false
                }
            });

            // Drain all pending messages
            loop {
                let message = match receiver.try_recv() {
                    Ok(msg) => msg,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                };

                if Self::handle_message(message, &mut dialogs, &handler) {
                    return;
                }
            }
        }
    }
}

impl AppKitBackend {
    /// Processes a single dialog message. Returns true when the event loop should exit.
    fn handle_message(
        message: DialogMessageRequest,
        dialogs: &mut HashMap<usize, AppKitDialog>,
        handler: &Retained<AnyObject>,
    ) -> bool {
        match message {
            DialogMessageRequest::None => {}
            DialogMessageRequest::ExitEventLoop => {
                for (_id, dialog) in dialogs.drain() {
                    dialog.close();
                }
                PROGRESS_CALLBACKS.lock().unwrap_or_else(|e| e.into_inner()).clear();
                return true;
            }
            DialogMessageRequest::CloseWindow(id) => {
                if let Some(dialog) = dialogs.remove(&id) {
                    dialog.close();
                    remove_progress_callback(id);
                    send_dialog_result(id, XDialogResult::WindowClosed);
                }
            }
            DialogMessageRequest::ShowMessageWindow(id, options, creation) => {
                let (dialog_sender, dialog_receiver) = oneshot::channel();
                RESULT_SENDERS.lock().unwrap_or_else(|e| e.into_inner()).insert(id, dialog_sender);
                let dialog = AppKitDialog::new(id, options, false, handler);
                dialog.show();
                dialogs.insert(id, dialog);
                let _ = creation.send(Ok(dialog_receiver));
            }
            DialogMessageRequest::ShowProgressWindow(id, options, creation, on_button) => {
                let (dialog_sender, dialog_receiver) = oneshot::channel();
                RESULT_SENDERS.lock().unwrap_or_else(|e| e.into_inner()).insert(id, dialog_sender);
                if let Some(cb) = on_button {
                    PROGRESS_CALLBACKS.lock().unwrap_or_else(|e| e.into_inner()).insert(id, cb);
                }
                let dialog = AppKitDialog::new(id, options, true, handler);
                dialog.show();
                dialogs.insert(id, dialog);
                let _ = creation.send(Ok(dialog_receiver));
            }
            DialogMessageRequest::SetProgressIndeterminate(id) => {
                if let Some(dialog) = dialogs.get_mut(&id) {
                    dialog.set_progress_indeterminate();
                }
            }
            DialogMessageRequest::SetProgressValue(id, value) => {
                if let Some(dialog) = dialogs.get_mut(&id) {
                    dialog.set_progress_value(value);
                }
            }
            DialogMessageRequest::SetProgressText(id, text) => {
                if let Some(dialog) = dialogs.get_mut(&id) {
                    dialog.set_body_text(&text);
                }
            }
        }
        false
    }
}
