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

struct MacCfDirectHandler {
    active: Arc<Mutex<HashMap<usize, NotificationPtr>>>,
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

                    active.lock().unwrap().insert(id, NotificationPtr(notification));

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
                if let Some(ptr) = self.active.lock().unwrap().get(&id) {
                    unsafe { CFUserNotificationCancel(ptr.0) };
                }
                Ok(())
            }
            DialogMessageRequest::ShowProgressWindow(_id, _options, creation_sender) => {
                let _ = creation_sender.send(Err(XDialogError::SystemError(
                    "Progress dialogs are not supported by the maccf-direct backend".to_string(),
                )));
                Ok(())
            }
            DialogMessageRequest::SetProgressValue(..)
            | DialogMessageRequest::SetProgressText(..)
            | DialogMessageRequest::SetProgressIndeterminate(..) => Err(XDialogError::SystemError(
                "Progress dialogs are not supported by the maccf-direct backend".to_string(),
            )),
            DialogMessageRequest::ExitEventLoop | DialogMessageRequest::None => Ok(()),
        }
    }
}

/// Initialize xdialog to use macOS CFUserNotification directly, without an event loop or
/// [`XDialogBuilder`]. This must be called before any dialog functions.
/// Can only be called once; subsequent calls will be ignored with a warning.
///
/// Note: This backend only supports message dialogs (up to 3 buttons).
/// Progress dialogs will return [`XDialogError::SystemError`] when attempted.
pub fn init_maccf_direct() {
    crate::channel::init_handler(Box::new(MacCfDirectHandler {
        active: Arc::new(Mutex::new(HashMap::new())),
    }));
}
