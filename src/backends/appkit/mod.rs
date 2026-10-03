mod appkit_dialog;

use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::NonNull;
use std::sync::mpsc::{Receiver, TryRecvError};

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
use objc2::{msg_send, sel, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventMask};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode};

use crate::model::*;
use crate::{ProgressButtonCallback, ProgressDialogProxy};

use appkit_dialog::AppKitDialog;

/// An open dialog: its window, result sender (the first result wins) and progress button callback.
struct Open {
    dialog: AppKitDialog,
    result: ResultSender,
    on_button: Option<ProgressButtonCallback>,
}

impl Open {
    /// Hide the window and deliver `result` (unless a result was delivered already).
    fn finish(&self, result: XDialogResult) {
        self.dialog.close();
        self.result.send(result);
    }
}

/// Low bits of a button's tag that hold the button index; the dialog id is in the bits above.
const TAG_INDEX_BITS: u32 = 16;

/// The tag of button `index` of dialog `id`, which identifies it to the click handler.
fn button_tag(id: usize, index: usize) -> isize {
    ((id << TAG_INDEX_BITS) | index) as isize
}

/// The `(dialog id, button index)` a tag from [`button_tag`] stands for.
fn split_tag(tag: isize) -> (usize, usize) {
    let tag = tag as usize;
    (tag >> TAG_INDEX_BITS, tag & ((1 << TAG_INDEX_BITS) - 1))
}

thread_local! {
    /// Open dialogs by id, on the AppKit loop thread. Global because the `buttonClicked:`
    /// handler is an extern "C" callback that can't capture Rust state: it finds the dialog by
    /// the button's tag.
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
    let (dialog_id, button_index) = split_tag(tag);

    // If a progress button callback is registered, it decides whether the dialog closes (it runs
    // under the borrow: xdialog calls from it only queue requests, which never touch OPEN).
    // Otherwise deliver the result and close. The window is only hidden here (it is the sender's):
    // the loop drops it with the next sweep of invisible windows. A panic must not unwind out of
    // this extern "C" fn (that aborts): it closes the dialog instead.
    OPEN.with_borrow_mut(|open| {
            let Some(o) = open.get_mut(&dialog_id) else { return };
            let keep_open = match o.on_button.as_mut() {
                Some(cb) => match catch_unwind(AssertUnwindSafe(|| cb(button_index, &ProgressDialogProxy::non_owning(dialog_id)))) {
                    Ok(keep_open) => keep_open,
                    Err(_) => {
                        error!("xdialog: a progress button callback panicked; closing the dialog");
                        o.on_button = None;
                        o.finish(XDialogResult::WindowClosed);
                        return;
                    }
                },
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
    // NSApplication and NSWindow are main-thread only; off it, every dialog is refused.
    let Some(mtm) = MainThreadMarker::new() else {
        error!("xdialog: the AppKit backend must run on the main thread; no dialogs will be shown");
        for message in receiver {
            if matches!(message, DialogMessageRequest::ExitEventLoop) {
                return;
            }
            crate::channel::reject(message);
        }
        return;
    };
    // Don't touch AppKit until a dialog is requested: connecting to the window server registers
    // with LaunchServices, and an executable inside another app's bundle (e.g. an updater in
    // Contents/MacOS) checks in as a second instance of that app, which can appear in the Dock
    // and steal focus.
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

    let app = NSApplication::sharedApplication(mtm);
    // the equivalent of LSUIElement: no Dock icon or menu bar, but windows can
    // still be shown and focused. Must be set before the app finishes launching.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();

    if handle_message(first_message, &handler, mtm) {
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

        loop {
            let message = match receiver.try_recv() {
                Ok(msg) => msg,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            };

            if handle_message(message, &handler, mtm) {
                return;
            }
        }
    }
}

/// Processes a single dialog message. Returns true when the event loop should exit.
fn handle_message(message: DialogMessageRequest, handler: &Retained<AnyObject>, mtm: MainThreadMarker) -> bool {
    match message {
        DialogMessageRequest::None => {}
        DialogMessageRequest::ExitEventLoop => {
            OPEN.take().values().for_each(|o| o.finish(XDialogResult::WindowClosed));
            return true;
        }
        DialogMessageRequest::CloseWindow(id) => {
            if let Some(o) = OPEN.with_borrow_mut(|open| open.remove(&id)) {
                o.finish(XDialogResult::WindowClosed);
            }
        }
        DialogMessageRequest::ShowMessageWindow(id, options, creation) => show(handler, mtm, id, options, false, creation, None),
        DialogMessageRequest::ShowProgressWindow(id, options, creation, on_button) => {
            show(handler, mtm, id, options, true, creation, on_button)
        }
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
        mtm: MainThreadMarker,
        id: usize,
        options: XDialogOptions,
        progress: bool,
        reply: DialogReply,
        on_button: Option<ProgressButtonCallback>) {
    let dialog = AppKitDialog::new(id, options, progress, handler, mtm);
    dialog.show();
    let result = reply.opened();
    OPEN.with_borrow_mut(|open| open.insert(id, Open { dialog, result, on_button }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_tag_round_trips() {
        assert_eq!(split_tag(button_tag(0, 0)), (0, 0));
        assert_eq!(split_tag(button_tag(7, 2)), (7, 2));
        assert_eq!(split_tag(button_tag(1 << 20, (1 << TAG_INDEX_BITS) - 1)), (1 << 20, (1 << TAG_INDEX_BITS) - 1));
    }
}
