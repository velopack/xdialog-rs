mod appkit_dialog;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
use objc2::{msg_send, sel};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventMask};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode};

use crate::model::*;
use crate::{ProgressButtonCallback, ProgressDialogProxy};

use appkit_dialog::AppKitDialog;

/// An open dialog: its window, result sender (`None` once the result was sent) and progress
/// button callback.
struct Open {
    dialog: AppKitDialog,
    result: Option<mpsc::Sender<XDialogResult>>,
    on_button: Option<ProgressButtonCallback>,
}

impl Open {
    /// Hide the window and deliver `result` (unless a result was delivered already).
    fn finish(&mut self, result: XDialogResult) {
        self.dialog.close();
        if let Some(tx) = self.result.take() {
            let _ = tx.send(result);
        }
    }
}

thread_local! {
    /// The open dialogs, keyed by dialog id, on the AppKit loop thread. Not passed around because
    /// the buttonClicked: handler is an extern "C" callback that can't capture Rust state: it looks
    /// the dialog up by the id in the button's tag.
    static OPEN: RefCell<HashMap<usize, Open>> = RefCell::new(HashMap::new());
}

/// A new instance of the button click handler class (registered on first use).
fn click_handler() -> Retained<AnyObject> {
    let cls = AnyClass::get(c"XDialogButtonClickHandler").unwrap_or_else(|| {
        let superclass = AnyClass::get(c"NSObject").unwrap();
        let mut builder = ClassBuilder::new(c"XDialogButtonClickHandler", superclass).unwrap();
        unsafe {
            builder.add_method(
                sel!(buttonClicked:),
                button_clicked as unsafe extern "C" fn(NonNull<AnyObject>, Sel, NonNull<AnyObject>),
            );
        }
        builder.register()
    });
    unsafe { msg_send![cls, new] }
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

    // If a progress button callback is registered, it decides whether the dialog closes (it runs
    // under the borrow: xdialog calls from it only queue requests, which never touch OPEN).
    // Otherwise deliver the result and close. The window is only hidden here (it is the sender's):
    // the loop drops it with the next sweep of invisible windows.
    OPEN.with_borrow_mut(|open| {
            let Some(o) = open.get_mut(&dialog_id) else { return };
            let keep_open = match o.on_button.as_mut() {
                Some(cb) => cb(button_index, &ProgressDialogProxy::non_owning(dialog_id)),
                None => false,
            };
            if !keep_open {
                o.finish(XDialogResult::ButtonPressed(button_index));
            }
        });
}

/// Serve `receiver` with AppKit on this thread until `ExitEventLoop`.
pub(crate) fn run_loop(receiver: Receiver<DialogMessageRequest>) {
    // Button callbacks run on this thread: a blocking `show_message*` there fails with
    // `BlockingCallOnUiThread` instead of deadlocking.
    let _ui_thread = crate::channel::UiThreadMark::set();
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

    let handler = click_handler();

    let app = unsafe {
        let app = NSApplication::sharedApplication(objc2::MainThreadMarker::new_unchecked());
        // the equivalent of LSUIElement: no Dock icon or menu bar, but windows can
        // still be shown and focused. Must be set before the app finishes launching.
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        app
    };

    if handle_message(first_message, &handler) {
        return;
    }

    loop {
        // Pump AppKit events until none arrives for 50ms
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

        // Forget closed windows (closed by the user: `WindowClosed`).
        OPEN.with_borrow_mut(|open| {
                open.retain(|_, o| {
                        o.dialog.is_visible() || {
                            o.finish(XDialogResult::WindowClosed);
                            false
                        }
                    })
            });

        // Drain all pending messages
        loop {
            let message = match receiver.try_recv() {
                Ok(msg) => msg,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            };

            if handle_message(message, &handler) {
                return;
            }
        }
    }
}

/// Processes a single dialog message. Returns true when the event loop should exit.
fn handle_message(message: DialogMessageRequest, handler: &Retained<AnyObject>) -> bool {
    match message {
        DialogMessageRequest::None => {}
        DialogMessageRequest::ExitEventLoop => {
            OPEN.take().values_mut().for_each(|o| o.finish(XDialogResult::WindowClosed));
            return true;
        }
        DialogMessageRequest::CloseWindow(id) => {
            if let Some(mut o) = OPEN.with_borrow_mut(|open| open.remove(&id)) {
                o.finish(XDialogResult::WindowClosed);
            }
        }
        DialogMessageRequest::ShowMessageWindow(id, options, creation) => show(handler, id, options, false, creation, None),
        DialogMessageRequest::ShowProgressWindow(id, options, creation, on_button) => show(handler, id, options, true, creation, on_button),
        DialogMessageRequest::SetProgressIndeterminate(id) => with_dialog(id, |d| d.set_progress_indeterminate()),
        DialogMessageRequest::SetProgressValue(id, value) => with_dialog(id, |d| d.set_progress_value(value)),
        DialogMessageRequest::SetProgressText(id, text) => with_dialog(id, |d| d.set_body_text(&text)),
    }
    false
}

/// Run `f` on dialog `id`, if it is open.
fn with_dialog(id: usize, f: impl FnOnce(&mut AppKitDialog)) {
    OPEN.with_borrow_mut(|open| open.get_mut(&id).map(|o| f(&mut o.dialog)));
}

fn show(handler: &Retained<AnyObject>,
        id: usize,
        options: XDialogOptions,
        progress: bool,
        creation: CreationSender,
        on_button: Option<ProgressButtonCallback>) {
    let dialog = AppKitDialog::new(id, options, progress, handler);
    dialog.show();
    let (tx, rx) = mpsc::channel();
    OPEN.with_borrow_mut(|open| open.insert(id, Open { dialog, result: Some(tx), on_button }));
    let _ = creation.send(Ok(rx));
}
