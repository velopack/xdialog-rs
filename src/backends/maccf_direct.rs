use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_foundation_sys::base::{CFOptionFlags, CFRelease, SInt32};
use core_foundation_sys::user_notification::*;

use crate::channel::DialogRequestHandler;
use crate::*;

struct NotificationPtr(CFUserNotificationRef);
unsafe impl Send for NotificationPtr {}
unsafe impl Sync for NotificationPtr {}

/// Width, in cells, of the text progress bar drawn for progress dialogs.
const BAR_WIDTH: usize = 10;
/// Width, in cells, of the moving segment used for the indeterminate animation.
const INDETERMINATE_SEG: usize = 2;
/// Filled / empty cell glyphs for the progress bar.
const CELL_FILLED: char = '●';
const CELL_EMPTY: char = '○';
/// Seconds between animation frames. This is also the interval at which a progress dialog polls
/// for button presses, so updates from `set_text`/`set_value` become visible within one tick.
const PROGRESS_TICK: f64 = 0.1;

fn icon_to_alert_level(icon: &XDialogIcon) -> CFOptionFlags {
    match icon {
        XDialogIcon::Error => kCFUserNotificationStopAlertLevel,
        XDialogIcon::Information => kCFUserNotificationNoteAlertLevel,
        XDialogIcon::Warning => kCFUserNotificationCautionAlertLevel,
        XDialogIcon::None => kCFUserNotificationPlainAlertLevel,
    }
}

fn build_notification_dict(options: &XDialogOptions) -> CFDictionary<CFString, CFString> {
    let mut pairs: Vec<(CFString, CFString)> = Vec::new();

    // Header: use main_instruction, fall back to title if empty
    let header = if options.main_instruction.is_empty() {
        &options.title
    } else {
        &options.main_instruction
    };
    unsafe {
        pairs.push((
            CFString::wrap_under_get_rule(kCFUserNotificationAlertHeaderKey),
            CFString::new(header),
        ));
    }

    // Message body
    if !options.message.is_empty() {
        unsafe {
            pairs.push((
                CFString::wrap_under_get_rule(kCFUserNotificationAlertMessageKey),
                CFString::new(&options.message),
            ));
        }
    }

    // Buttons (up to 3)
    let button_keys = unsafe {
        [
            kCFUserNotificationDefaultButtonTitleKey,
            kCFUserNotificationAlternateButtonTitleKey,
            kCFUserNotificationOtherButtonTitleKey,
        ]
    };
    for (i, button) in options.buttons.iter().take(3).enumerate() {
        unsafe {
            pairs.push((
                CFString::wrap_under_get_rule(button_keys[i]),
                CFString::new(button),
            ));
        }
    }

    CFDictionary::from_CFType_pairs(&pairs)
}

/// How the text progress bar is currently rendered.
#[derive(Clone, Copy)]
enum ProgressMode {
    /// A filled bar at the given fraction (0.0..=1.0).
    Determinate(f32),
    /// A segment that bounces back and forth across the bar.
    Indeterminate,
}

/// Renders the unicode progress bar for the current mode and animation frame.
fn render_bar(mode: ProgressMode, frame: usize) -> String {
    match mode {
        ProgressMode::Determinate(value) => {
            let v = value.clamp(0.0, 1.0);
            let filled = (v * BAR_WIDTH as f32).round() as usize;
            let mut s = String::new();
            for i in 0..BAR_WIDTH {
                s.push(if i < filled { CELL_FILLED } else { CELL_EMPTY });
            }
            s
        }
        ProgressMode::Indeterminate => {
            // Bounce a segment of width INDETERMINATE_SEG between the two ends of the bar.
            let span = BAR_WIDTH - INDETERMINATE_SEG;
            let period = span * 2;
            let p = frame % period;
            let pos = if p <= span { p } else { period - p };
            let mut s = String::new();
            for i in 0..BAR_WIDTH {
                s.push(if i >= pos && i < pos + INDETERMINATE_SEG { CELL_FILLED } else { CELL_EMPTY });
            }
            s
        }
    }
}

/// Composes the dialog body: the caller's text, a blank line, then the progress bar.
fn compose_progress_message(body: &str, bar: &str) -> String {
    if body.is_empty() {
        bar.to_string()
    } else {
        format!("{}\n\n{}", body, bar)
    }
}

/// Mutable state shared between the animation thread that owns a progress dialog and the request
/// handler, which mutates it in response to `SetProgress*`/`CloseWindow` from other threads.
struct ProgressState {
    /// The live notification. Replaced if the dialog has to be recreated (see the keep-open path).
    notification: NotificationPtr,
    icon_flags: CFOptionFlags,
    header: String,
    buttons: Vec<String>,
    /// Body text set via `set_text`; the progress bar is appended below it on render.
    body: String,
    mode: ProgressMode,
    /// Set when the body/value/mode changed so a determinate dialog re-renders on the next tick.
    dirty: bool,
    /// Set by `CloseWindow` to ask the animation thread to exit.
    closed: bool,
}

struct ProgressShared {
    state: Mutex<ProgressState>,
}

/// A dialog tracked by the handler. Message dialogs only need their notification pointer to be
/// cancellable; progress dialogs carry shared state the animation thread renders from.
enum Active {
    Message(NotificationPtr),
    Progress(Arc<ProgressShared>),
}

struct MacCfDirectHandler {
    active: Arc<Mutex<HashMap<usize, Active>>>,
}

impl MacCfDirectHandler {
    /// Applies `f` to the shared state of the progress dialog with the given id, if one exists.
    fn update_progress<F: FnOnce(&mut ProgressState)>(&self, id: usize, f: F) {
        let guard = self.active.lock().unwrap();
        if let Some(Active::Progress(shared)) = guard.get(&id) {
            let mut st = shared.state.lock().unwrap();
            f(&mut st);
        }
    }
}

impl DialogRequestHandler for MacCfDirectHandler {
    fn send(&self, message: DialogMessageRequest) -> Result<(), XDialogError> {
        match message {
            DialogMessageRequest::ShowMessageWindow(id, options, creation_sender) => {
                let active = Arc::clone(&self.active);
                let (dialog_sender, dialog_receiver) = oneshot::channel();
                let _ = creation_sender.send(Ok(dialog_receiver));

                std::thread::spawn(move || {
                    let flags = icon_to_alert_level(&options.icon);
                    let dict = build_notification_dict(&options);
                    let mut error: SInt32 = 0;

                    let notification = unsafe {
                        CFUserNotificationCreate(
                            std::ptr::null(),
                            0.0,
                            flags,
                            &mut error,
                            dict.as_concrete_TypeRef(),
                        )
                    };

                    if notification.is_null() || error != 0 {
                        let _ = dialog_sender.send(XDialogResult::WindowClosed);
                        return;
                    }

                    active.lock().unwrap().insert(id, Active::Message(NotificationPtr(notification)));

                    let mut response_flags: CFOptionFlags = 0;
                    unsafe {
                        CFUserNotificationReceiveResponse(notification, 0.0, &mut response_flags);
                    }

                    active.lock().unwrap().remove(&id);

                    let response = response_flags & 0x3;
                    let result = if response == kCFUserNotificationDefaultResponse {
                        XDialogResult::ButtonPressed(0)
                    } else if response == kCFUserNotificationAlternateResponse {
                        XDialogResult::ButtonPressed(1)
                    } else if response == kCFUserNotificationOtherResponse {
                        XDialogResult::ButtonPressed(2)
                    } else {
                        XDialogResult::WindowClosed
                    };

                    let _ = dialog_sender.send(result);

                    unsafe { CFRelease(notification as *const _) };
                });

                Ok(())
            }
            DialogMessageRequest::CloseWindow(id) => {
                let guard = self.active.lock().unwrap();
                match guard.get(&id) {
                    Some(Active::Message(ptr)) => unsafe {
                        CFUserNotificationCancel(ptr.0);
                    },
                    Some(Active::Progress(shared)) => {
                        // Mark closed and cancel while holding the lock so the animation thread
                        // can't release the notification out from under us.
                        let mut st = shared.state.lock().unwrap();
                        st.closed = true;
                        unsafe { CFUserNotificationCancel(st.notification.0) };
                    }
                    None => {}
                }
                Ok(())
            }
            DialogMessageRequest::ShowProgressWindow(id, options, creation_sender, on_button) => {
                let active = Arc::clone(&self.active);
                std::thread::spawn(move || {
                    run_progress_dialog(id, options, on_button, active, creation_sender);
                });
                Ok(())
            }
            DialogMessageRequest::SetProgressValue(id, value) => {
                self.update_progress(id, |st| {
                    st.mode = ProgressMode::Determinate(value);
                    st.dirty = true;
                });
                Ok(())
            }
            DialogMessageRequest::SetProgressIndeterminate(id) => {
                self.update_progress(id, |st| {
                    st.mode = ProgressMode::Indeterminate;
                    st.dirty = true;
                });
                Ok(())
            }
            DialogMessageRequest::SetProgressText(id, text) => {
                self.update_progress(id, |st| {
                    st.body = text;
                    st.dirty = true;
                });
                Ok(())
            }
            DialogMessageRequest::ExitEventLoop | DialogMessageRequest::None => Ok(()),
        }
    }
}

/// Builds the notification dictionary for a progress dialog from its current state and frame.
fn build_progress_dict(state: &ProgressState, frame: usize) -> CFDictionary<CFString, CFString> {
    let mut pairs: Vec<(CFString, CFString)> = Vec::new();

    unsafe {
        pairs.push((
            CFString::wrap_under_get_rule(kCFUserNotificationAlertHeaderKey),
            CFString::new(&state.header),
        ));
    }

    let bar = render_bar(state.mode, frame);
    let message = compose_progress_message(&state.body, &bar);
    unsafe {
        pairs.push((
            CFString::wrap_under_get_rule(kCFUserNotificationAlertMessageKey),
            CFString::new(&message),
        ));
    }

    let button_keys = unsafe {
        [
            kCFUserNotificationDefaultButtonTitleKey,
            kCFUserNotificationAlternateButtonTitleKey,
            kCFUserNotificationOtherButtonTitleKey,
        ]
    };
    for (i, button) in state.buttons.iter().take(3).enumerate() {
        unsafe {
            pairs.push((
                CFString::wrap_under_get_rule(button_keys[i]),
                CFString::new(button),
            ));
        }
    }

    CFDictionary::from_CFType_pairs(&pairs)
}

/// Owns a progress dialog for its lifetime: creates the notification, renders the animated text
/// bar, polls for button presses, and tears everything down on close. Runs on its own thread.
fn run_progress_dialog(
    id: usize,
    options: XDialogOptions,
    mut on_button: Option<ProgressButtonCallback>,
    active: Arc<Mutex<HashMap<usize, Active>>>,
    creation_sender: CreationSender,
) {
    let icon_flags = icon_to_alert_level(&options.icon);
    let header = if options.main_instruction.is_empty() {
        options.title.clone()
    } else {
        options.main_instruction.clone()
    };

    // Default to determinate at 0, matching the other backends' initial progress state.
    let mut state = ProgressState {
        notification: NotificationPtr(std::ptr::null_mut()),
        icon_flags,
        header,
        buttons: options.buttons.clone(),
        body: options.message.clone(),
        mode: ProgressMode::Determinate(0.0),
        dirty: false,
        closed: false,
    };

    let dict = build_progress_dict(&state, 0);
    let mut error: SInt32 = 0;
    let notification = unsafe {
        CFUserNotificationCreate(std::ptr::null(), 0.0, icon_flags, &mut error, dict.as_concrete_TypeRef())
    };
    if notification.is_null() || error != 0 {
        let _ = creation_sender.send(Err(XDialogError::SystemError(
            "Failed to create CFUserNotification progress dialog".to_string(),
        )));
        return;
    }
    state.notification = NotificationPtr(notification);

    let shared = Arc::new(ProgressShared { state: Mutex::new(state) });
    active.lock().unwrap().insert(id, Active::Progress(Arc::clone(&shared)));

    let (dialog_sender, dialog_receiver) = oneshot::channel();
    let _ = creation_sender.send(Ok(dialog_receiver));

    let result = run_progress_loop(&shared, &mut on_button, id);

    // Remove from the active map (under lock) before releasing the notification, so a concurrent
    // CloseWindow cannot cancel a notification we are about to free.
    active.lock().unwrap().remove(&id);
    let final_ptr = shared.state.lock().unwrap().notification.0;
    let _ = dialog_sender.send(result);
    unsafe { CFRelease(final_ptr as *const _) };
}

/// Drives the render/poll loop until the dialog closes. Returns the result to report to any caller
/// awaiting the dialog (progress callers normally discard it).
fn run_progress_loop(
    shared: &Arc<ProgressShared>,
    on_button: &mut Option<ProgressButtonCallback>,
    id: usize,
) -> XDialogResult {
    let mut frame: usize = 0;
    loop {
        // Render the current state, then wait up to one tick for a button press.
        let (notification, indeterminate) = {
            let mut st = shared.state.lock().unwrap();
            if st.closed {
                return XDialogResult::WindowClosed;
            }
            let indeterminate = matches!(st.mode, ProgressMode::Indeterminate);
            // Indeterminate redraws every tick to animate; determinate only when something changed.
            if indeterminate || st.dirty {
                let dict = build_progress_dict(&st, frame);
                unsafe {
                    CFUserNotificationUpdate(st.notification.0, 0.0, st.icon_flags, dict.as_concrete_TypeRef());
                }
                st.dirty = false;
            }
            (st.notification.0, indeterminate)
        };

        let mut response_flags: CFOptionFlags = 0;
        let ret = unsafe { CFUserNotificationReceiveResponse(notification, PROGRESS_TICK, &mut response_flags) };

        if ret != 0 {
            // Timed out with no response: advance the animation and loop.
            if indeterminate {
                frame = frame.wrapping_add(1);
            }
            continue;
        }

        let response = response_flags & 0x3;
        if response == kCFUserNotificationCancelResponse {
            return XDialogResult::WindowClosed;
        }
        let button_index = if response == kCFUserNotificationDefaultResponse {
            0
        } else if response == kCFUserNotificationAlternateResponse {
            1
        } else if response == kCFUserNotificationOtherResponse {
            2
        } else {
            return XDialogResult::WindowClosed;
        };

        let keep_open = match on_button {
            Some(cb) => {
                let proxy = ProgressDialogProxy::non_owning(id);
                (cb.0)(button_index, &proxy)
            }
            None => false,
        };

        if !keep_open {
            return XDialogResult::ButtonPressed(button_index);
        }

        // CFUserNotification dismisses itself when a button is clicked, so to honor keep-open we
        // recreate it from the (possibly callback-updated) state and keep going.
        if !recreate_progress_notification(shared, frame) {
            return XDialogResult::WindowClosed;
        }
    }
}

/// Recreates a progress dialog's notification in place (used after a button click when the caller
/// wants the dialog to stay open). Returns false if recreation failed or a close was requested.
fn recreate_progress_notification(shared: &Arc<ProgressShared>, frame: usize) -> bool {
    let mut st = shared.state.lock().unwrap();
    if st.closed {
        return false;
    }
    let dict = build_progress_dict(&st, frame);
    let mut error: SInt32 = 0;
    let new_notification = unsafe {
        CFUserNotificationCreate(std::ptr::null(), 0.0, st.icon_flags, &mut error, dict.as_concrete_TypeRef())
    };
    if new_notification.is_null() || error != 0 {
        return false;
    }
    let old = st.notification.0;
    st.notification = NotificationPtr(new_notification);
    unsafe {
        CFUserNotificationCancel(old);
        CFRelease(old as *const _);
    }
    true
}

/// Initialize xdialog to use macOS CFUserNotification directly, without an event loop or
/// [`XDialogBuilder`]. This must be called before any dialog functions.
/// Can only be called once; subsequent calls will be ignored with a warning.
///
/// Supports both message dialogs (up to 3 buttons) and progress dialogs. Because CFUserNotification
/// has no native progress control, progress is drawn as an animated unicode text bar in the dialog
/// body (determinate fills the bar to the current value; indeterminate bounces a segment).
pub fn init_maccf_direct() {
    crate::channel::init_handler(Box::new(MacCfDirectHandler {
        active: Arc::new(Mutex::new(HashMap::new())),
    }));
}
