mod appkit_dialog;

use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::mpsc::{Receiver, TryRecvError};

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
use objc2::{msg_send, sel};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventMask};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode};

use crate::backends::XDialogBackendImpl;
use crate::model::*;
use crate::state::insert_result;

use appkit_dialog::AppKitDialog;

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
    insert_result(dialog_id, XDialogResult::ButtonPressed(button_index));
    let window: Option<Retained<AnyObject>> = unsafe { msg_send![sender, window] };
    if let Some(window) = window {
        let () = unsafe { msg_send![&*window, orderOut: std::ptr::null::<AnyObject>()] };
    }
}

fn create_handler_instance() -> Retained<AnyObject> {
    let cls = AnyClass::get(c"XDialogButtonClickHandler").unwrap();
    unsafe { msg_send![cls, new] }
}

impl XDialogBackendImpl for AppKitBackend {
    fn run_loop(receiver: Receiver<DialogMessageRequest>, _theme: XDialogTheme) {
        register_button_handler_class();
        let handler = create_handler_instance();

        let app = unsafe {
            let app = NSApplication::sharedApplication(objc2::MainThreadMarker::new_unchecked());
            app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            app.finishLaunching();
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
            app
        };

        let mut dialogs: HashMap<usize, AppKitDialog> = HashMap::new();

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
                    insert_result(*id, XDialogResult::WindowClosed);
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

                match message {
                    DialogMessageRequest::None => {}
                    DialogMessageRequest::ExitEventLoop => {
                        for (_id, dialog) in dialogs.drain() {
                            dialog.close();
                        }
                        return;
                    }
                    DialogMessageRequest::CloseWindow(id) => {
                        if let Some(dialog) = dialogs.remove(&id) {
                            dialog.close();
                            insert_result(id, XDialogResult::WindowClosed);
                        }
                    }
                    DialogMessageRequest::ShowMessageWindow(id, options, mut result) => {
                        let dialog = AppKitDialog::new(id, options, false, &handler);
                        dialog.show();
                        dialogs.insert(id, dialog);
                        result.send_ok();
                    }
                    DialogMessageRequest::ShowProgressWindow(id, options, mut result) => {
                        let dialog = AppKitDialog::new(id, options, true, &handler);
                        dialog.show();
                        dialogs.insert(id, dialog);
                        result.send_ok();
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
            }
        }
    }
}
