use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};

use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_foundation_sys::base::{CFOptionFlags, CFRelease, SInt32};
use core_foundation_sys::user_notification::*;

use crate::channel::DialogRequestHandler;
use crate::*;

/// A dialog thread's `CFUserNotificationRef`; null until the thread has created it.
struct NotificationPtr(CFUserNotificationRef);
unsafe impl Send for NotificationPtr {}

impl NotificationPtr {
    const NULL: Self = NotificationPtr(std::ptr::null_mut());

    /// Dismisses the alert once it exists: its thread's `ReceiveResponse` returns a cancel response.
    fn cancel(&self) {
        if !self.0.is_null() {
            unsafe { CFUserNotificationCancel(self.0) };
        }
    }
}

/// `mach_msg` timed out: what `CFUserNotificationReceiveResponse` returns when its timeout elapses
/// without a response (`mach/message.h`).
const MACH_RCV_TIMED_OUT: SInt32 = 0x1000_4003;
/// Consecutive `ReceiveResponse` failures (other than a timeout) after which a progress dialog is
/// given up instead of spinning.
const MAX_RECEIVE_ERRORS: u32 = 10;

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
        // `Custom` is only shown by the drawn backends (Fluent, Ubuntu, MacOS).
        XDialogIcon::None | XDialogIcon::Custom => kCFUserNotificationPlainAlertLevel,
    }
}

/// The alert header: `main_instruction`, or `title` when that is empty.
fn header(options: &XDialogOptions) -> &str {
    if options.main_instruction.is_empty() {
        &options.title
    } else {
        &options.main_instruction
    }
}

/// The notification dictionary of an alert. CF has three button slots; the crate's last button is
/// the default (Return) button, the one before it the alternate, the one before that the other, so
/// with more than three buttons only the last three are shown (see [`pressed_button`]).
fn alert_dict(header: &str, message: &str, buttons: &[String]) -> CFDictionary<CFString, CFString> {
    let mut pairs: Vec<(CFString, CFString)> = Vec::new();
    unsafe {
        pairs.push((CFString::wrap_under_get_rule(kCFUserNotificationAlertHeaderKey), CFString::new(header)));
        if !message.is_empty() {
            pairs.push((CFString::wrap_under_get_rule(kCFUserNotificationAlertMessageKey), CFString::new(message)));
        }
        let keys = [kCFUserNotificationDefaultButtonTitleKey,
                    kCFUserNotificationAlternateButtonTitleKey,
                    kCFUserNotificationOtherButtonTitleKey];
        for (key, button) in keys.into_iter().zip(buttons.iter().rev()) {
            pairs.push((CFString::wrap_under_get_rule(key), CFString::new(button)));
        }
    }
    CFDictionary::from_CFType_pairs(&pairs)
}

/// The crate button index answered by `response_flags` for a dialog with `button_count` buttons,
/// `None` for a cancel (or a response to a button the dialog does not have: CF's implicit OK of a
/// dialog without buttons). Inverse of the slot assignment in [`alert_dict`].
fn pressed_button(response_flags: CFOptionFlags, button_count: usize) -> Option<usize> {
    let responses = [kCFUserNotificationDefaultResponse, kCFUserNotificationAlternateResponse, kCFUserNotificationOtherResponse];
    let slot = responses.iter().position(|&r| r == response_flags & 0x3)?;
    button_count.checked_sub(slot + 1)
}

/// Creates (and shows) a notification; the error carries CF's error code.
fn create(flags: CFOptionFlags, dict: &CFDictionary<CFString, CFString>) -> Result<NotificationPtr, XDialogError> {
    let mut error: SInt32 = 0;
    let notification = unsafe { CFUserNotificationCreate(std::ptr::null(), 0.0, flags, &mut error, dict.as_concrete_TypeRef()) };
    if notification.is_null() || error != 0 {
        if !notification.is_null() {
            unsafe { CFRelease(notification as *const _) };
        }
        return Err(XDialogError::SystemError(format!("CFUserNotificationCreate failed ({error})")));
    }
    Ok(NotificationPtr(notification))
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
    let lit = match mode {
        ProgressMode::Determinate(value) => 0..(value.clamp(0.0, 1.0) * BAR_WIDTH as f32).round() as usize,
        ProgressMode::Indeterminate => {
            // Bounce a segment of width INDETERMINATE_SEG between the two ends of the bar.
            let span = BAR_WIDTH - INDETERMINATE_SEG;
            let period = span * 2;
            let p = frame % period;
            let pos = if p <= span { p } else { period - p };
            pos..pos + INDETERMINATE_SEG
        }
    };
    (0..BAR_WIDTH).map(|i| if lit.contains(&i) { CELL_FILLED } else { CELL_EMPTY }).collect()
}

/// Composes the dialog body: the caller's text, a blank line, then the progress bar.
fn compose_progress_message(body: &str, bar: &str) -> String {
    if body.is_empty() {
        bar.to_string()
    } else {
        format!("{}\n\n{}", body, bar)
    }
}

/// A message dialog as seen by the handler: `closed` is set by `CloseWindow`, which may arrive
/// before the dialog thread has created the notification.
struct MessageSlot {
    notification: NotificationPtr,
    closed: bool,
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

impl ProgressState {
    /// The initial state of a progress dialog: determinate at 0, matching the other backends.
    fn new(options: XDialogOptions) -> Self {
        // Without a default button title CF adds an "OK" button, which would let the user dismiss
        // a dialog that is meant to have no buttons.
        let mut icon_flags = icon_to_alert_level(&options.icon);
        if options.buttons.is_empty() {
            icon_flags |= kCFUserNotificationNoDefaultButtonFlag;
        }
        ProgressState { notification: NotificationPtr::NULL,
                        icon_flags,
                        header: header(&options).to_string(),
                        buttons: options.buttons,
                        body: options.message,
                        mode: ProgressMode::Determinate(0.0),
                        dirty: false,
                        closed: false }
    }
}

/// A dialog tracked by the handler, registered before its thread starts so a `CloseWindow` can
/// never miss it. Message dialogs only need their notification to be cancellable; progress dialogs
/// carry the state the animation thread renders from.
enum Active {
    Message(Arc<Mutex<MessageSlot>>),
    Progress(Arc<Mutex<ProgressState>>),
}

type ActiveMap = Arc<Mutex<HashMap<usize, Active>>>;

struct MacCfDirectHandler {
    active: ActiveMap,
}

impl MacCfDirectHandler {
    /// Applies `f` to the shared state of the progress dialog with the given id, if one exists.
    fn update_progress<F: FnOnce(&mut ProgressState)>(&self, id: usize, f: F) {
        let guard = self.active.lock().unwrap();
        if let Some(Active::Progress(shared)) = guard.get(&id) {
            f(&mut shared.lock().unwrap());
        }
    }
}

impl DialogRequestHandler for MacCfDirectHandler {
    fn send(&self, message: DialogMessageRequest) -> Result<(), XDialogError> {
        match message {
            DialogMessageRequest::ShowMessageWindow(id, options, reply) => {
                let slot = Arc::new(Mutex::new(MessageSlot { notification: NotificationPtr::NULL, closed: false }));
                self.active.lock().unwrap().insert(id, Active::Message(Arc::clone(&slot)));
                let active = Arc::clone(&self.active);
                std::thread::spawn(move || {
                    run_message_dialog(id, options, slot, active, reply);
                });
                Ok(())
            }
            DialogMessageRequest::CloseWindow(id) => {
                // Mark closed and cancel while holding the lock so the dialog thread can't release
                // the notification out from under us.
                let guard = self.active.lock().unwrap();
                match guard.get(&id) {
                    Some(Active::Message(slot)) => {
                        let mut st = slot.lock().unwrap();
                        st.closed = true;
                        st.notification.cancel();
                    }
                    Some(Active::Progress(shared)) => {
                        let mut st = shared.lock().unwrap();
                        st.closed = true;
                        st.notification.cancel();
                    }
                    None => {}
                }
                Ok(())
            }
            DialogMessageRequest::ShowProgressWindow(id, options, reply, on_button) => {
                let shared = Arc::new(Mutex::new(ProgressState::new(options)));
                self.active.lock().unwrap().insert(id, Active::Progress(Arc::clone(&shared)));
                let active = Arc::clone(&self.active);
                std::thread::spawn(move || {
                    run_progress_dialog(id, shared, on_button, active, reply);
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

/// Owns a message dialog for its lifetime: shows the alert, waits for the answer and reports it.
/// Runs on its own thread; the handler registered `slot` under `id` before starting it.
fn run_message_dialog(id: usize,
                      options: XDialogOptions,
                      slot: Arc<Mutex<MessageSlot>>,
                      active: ActiveMap,
                      reply: DialogReply) {
    let dict = alert_dict(header(&options), &options.message, &options.buttons);
    let notification = match create(icon_to_alert_level(&options.icon), &dict) {
        Ok(notification) => notification.0,
        Err(e) => {
            active.lock().unwrap().remove(&id);
            reply.failed(e);
            return;
        }
    };
    {
        // A CloseWindow that arrived while the alert was being created dismisses it now.
        let mut st = slot.lock().unwrap();
        st.notification = NotificationPtr(notification);
        if st.closed {
            st.notification.cancel();
        }
    }
    let dialog_sender = reply.opened();

    let mut response_flags: CFOptionFlags = 0;
    let ret = unsafe { CFUserNotificationReceiveResponse(notification, 0.0, &mut response_flags) };

    // Remove from the active map (under lock) before releasing the notification, so a concurrent
    // CloseWindow cannot cancel a notification we are about to free.
    active.lock().unwrap().remove(&id);

    let result = if ret != 0 {
        warn!("xdialog: CFUserNotificationReceiveResponse failed ({ret})");
        XDialogResult::WindowClosed
    } else {
        pressed_button(response_flags, options.buttons.len()).map_or(XDialogResult::WindowClosed, XDialogResult::ButtonPressed)
    };
    dialog_sender.send(result);

    unsafe { CFRelease(notification as *const _) };
}

/// Builds the notification dictionary for a progress dialog from its current state and frame.
fn progress_dict(state: &ProgressState, frame: usize) -> CFDictionary<CFString, CFString> {
    let message = compose_progress_message(&state.body, &render_bar(state.mode, frame));
    alert_dict(&state.header, &message, &state.buttons)
}

/// Owns a progress dialog for its lifetime: creates the notification, renders the animated text
/// bar, polls for button presses, and tears everything down on close. Runs on its own thread;
/// the handler registered `shared` under `id` before starting it.
fn run_progress_dialog(id: usize,
                       shared: Arc<Mutex<ProgressState>>,
                       mut on_button: Option<ProgressButtonCallback>,
                       active: ActiveMap,
                       reply: DialogReply) {
    let (icon_flags, dict) = {
        let st = shared.lock().unwrap();
        (st.icon_flags, progress_dict(&st, 0))
    };
    match create(icon_flags, &dict) {
        Ok(notification) => {
            // A CloseWindow that arrived while the dialog was being created dismisses it now; the
            // loop then exits on `closed`.
            let mut st = shared.lock().unwrap();
            st.notification = notification;
            if st.closed {
                st.notification.cancel();
            }
        }
        Err(e) => {
            active.lock().unwrap().remove(&id);
            reply.failed(e);
            return;
        }
    }

    let dialog_sender = reply.opened();

    let result = run_progress_loop(&shared, &mut on_button, id);

    // Remove from the active map (under lock) before releasing the notification, so a concurrent
    // CloseWindow cannot cancel a notification we are about to free.
    active.lock().unwrap().remove(&id);
    let final_ptr = shared.lock().unwrap().notification.0;
    dialog_sender.send(result);
    unsafe { CFRelease(final_ptr as *const _) };
}

/// Drives the render/poll loop until the dialog closes. Returns the result to report to any caller
/// awaiting the dialog (progress callers normally discard it).
fn run_progress_loop(shared: &Mutex<ProgressState>, on_button: &mut Option<ProgressButtonCallback>, id: usize) -> XDialogResult {
    let mut frame: usize = 0;
    let mut errors: u32 = 0;
    loop {
        // Render the current state, then wait up to one tick for a button press.
        let (notification, indeterminate, button_count) = {
            let mut st = shared.lock().unwrap();
            if st.closed {
                return XDialogResult::WindowClosed;
            }
            let indeterminate = matches!(st.mode, ProgressMode::Indeterminate);
            // Indeterminate redraws every tick to animate; determinate only when something changed.
            if indeterminate || st.dirty {
                let dict = progress_dict(&st, frame);
                unsafe {
                    CFUserNotificationUpdate(st.notification.0, 0.0, st.icon_flags, dict.as_concrete_TypeRef());
                }
                st.dirty = false;
            }
            (st.notification.0, indeterminate, st.buttons.len())
        };

        let mut response_flags: CFOptionFlags = 0;
        let ret = unsafe { CFUserNotificationReceiveResponse(notification, PROGRESS_TICK, &mut response_flags) };

        if ret == MACH_RCV_TIMED_OUT {
            // No response within the tick: advance the animation and loop.
            errors = 0;
            if indeterminate {
                frame = frame.wrapping_add(1);
            }
            continue;
        }
        if ret != 0 {
            errors += 1;
            if errors < MAX_RECEIVE_ERRORS {
                continue;
            }
            warn!("xdialog: CFUserNotificationReceiveResponse failed ({ret}); closing the progress dialog");
            return XDialogResult::WindowClosed;
        }

        let Some(button_index) = pressed_button(response_flags, button_count) else {
            return XDialogResult::WindowClosed;
        };

        let keep_open = match on_button {
            Some(cb) => {
                let proxy = ProgressDialogProxy::non_owning(id);
                match catch_unwind(AssertUnwindSafe(|| cb(button_index, &proxy))) {
                    Ok(keep_open) => keep_open,
                    Err(_) => {
                        error!("xdialog: a progress button callback panicked; closing the dialog");
                        return XDialogResult::WindowClosed;
                    }
                }
            }
            None => false,
        };

        if !keep_open {
            return XDialogResult::ButtonPressed(button_index);
        }

        // CFUserNotification dismisses itself when a button is clicked, so to honor keep-open we
        // recreate it from the (possibly callback-updated) state and keep going.
        let mut st = shared.lock().unwrap();
        if st.closed {
            return XDialogResult::WindowClosed;
        }
        if let Err(e) = recreate_progress_notification(&mut st, frame) {
            warn!("xdialog: could not recreate the progress dialog: {e}");
            return XDialogResult::WindowClosed;
        }
    }
}

/// Replaces a progress dialog's (dismissed) notification with a new one rendered from `st`.
fn recreate_progress_notification(st: &mut ProgressState, frame: usize) -> Result<(), XDialogError> {
    let new_notification = create(st.icon_flags, &progress_dict(st, frame))?;
    let old = std::mem::replace(&mut st.notification, new_notification);
    old.cancel();
    unsafe { CFRelease(old.0 as *const _) };
    Ok(())
}

/// Initialize xdialog to use macOS CFUserNotification directly, without an event loop or
/// [`XDialogBuilder`]. This must be called before any dialog functions.
/// Can only be called once; subsequent calls will be ignored with a warning.
///
/// Supports both message dialogs and progress dialogs. CFUserNotification alerts have at most
/// three buttons: the last button of [`XDialogOptions::buttons`] is the default (Return) button
/// and, with more than three buttons, only the last three are shown. A message dialog without
/// buttons shows an OK button whose press is reported as [`XDialogResult::WindowClosed`]. Because
/// CFUserNotification has no native progress control, progress is drawn as an animated unicode
/// text bar in the dialog body (determinate fills the bar to the current value; indeterminate
/// bounces a segment).
pub fn init_maccf_direct() {
    crate::channel::init_handler(Box::new(MacCfDirectHandler { active: Arc::new(Mutex::new(HashMap::new())) }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(bar: &str) -> usize {
        bar.chars().filter(|&c| c == CELL_FILLED).count()
    }

    #[test]
    fn determinate_bar_fills_to_the_value() {
        let bar = render_bar(ProgressMode::Determinate(0.5), 0);
        assert_eq!(bar.chars().count(), BAR_WIDTH);
        assert_eq!(filled(&bar), BAR_WIDTH / 2);
        assert!(bar.starts_with(CELL_FILLED) && bar.ends_with(CELL_EMPTY));
        assert_eq!(filled(&render_bar(ProgressMode::Determinate(-1.0), 0)), 0);
        assert_eq!(filled(&render_bar(ProgressMode::Determinate(2.0), 0)), BAR_WIDTH);
    }

    #[test]
    fn indeterminate_bar_stays_within_bounds_over_a_full_period() {
        let span = BAR_WIDTH - INDETERMINATE_SEG;
        for frame in 0..span * 2 + 1 {
            let bar = render_bar(ProgressMode::Indeterminate, frame);
            assert_eq!(bar.chars().count(), BAR_WIDTH, "frame {frame}");
            assert_eq!(filled(&bar), INDETERMINATE_SEG, "frame {frame}");
        }
        assert!(render_bar(ProgressMode::Indeterminate, 0).starts_with(CELL_FILLED));
        assert!(render_bar(ProgressMode::Indeterminate, span).ends_with(CELL_FILLED));
    }

    #[test]
    fn progress_message_omits_the_blank_line_without_body() {
        assert_eq!(compose_progress_message("", "bar"), "bar");
        assert_eq!(compose_progress_message("body", "bar"), "body\n\nbar");
    }

    #[test]
    fn pressed_button_maps_cf_slots_from_the_last_button() {
        assert_eq!(pressed_button(kCFUserNotificationDefaultResponse, 3), Some(2));
        assert_eq!(pressed_button(kCFUserNotificationAlternateResponse, 3), Some(1));
        assert_eq!(pressed_button(kCFUserNotificationOtherResponse, 3), Some(0));
        assert_eq!(pressed_button(kCFUserNotificationCancelResponse, 3), None);
        // Only the last three of five buttons are shown.
        assert_eq!(pressed_button(kCFUserNotificationOtherResponse, 5), Some(2));
        // CF's implicit OK of a dialog without buttons; higher flag bits are ignored.
        assert_eq!(pressed_button(kCFUserNotificationDefaultResponse, 0), None);
        assert_eq!(pressed_button(kCFUserNotificationAlternateResponse | (1 << 8), 2), Some(0));
    }

    #[test]
    fn header_falls_back_to_the_title() {
        let mut options = XDialogOptions { title: "Title".into(), ..Default::default() };
        assert_eq!(header(&options), "Title");
        options.main_instruction = "Main".into();
        assert_eq!(header(&options), "Main");
    }
}
