use crate::errors::*;
use crate::model::*;
use crate::state::*;

/// Shows a message box with an information icon and an OK button and blocks until the user closes it.
pub fn show_message_info_ok<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    window_title: P1,
    main_instruction: P2,
    message: P3,
) -> Result<(), XDialogError> {
    show_message_internal(XDialogIcon::Information, window_title, main_instruction, message, vec!["OK".to_string()])?;
    Ok(())
}

/// Shows a message box with a warning icon and an OK button and blocks until the user closes it.
pub fn show_message_warn_ok<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    window_title: P1,
    main_instruction: P2,
    message: P3,
) -> Result<(), XDialogError> {
    show_message_internal(XDialogIcon::Warning, window_title, main_instruction, message, vec!["OK".to_string()])?;
    Ok(())
}

/// Shows a message box with an error icon and an OK button and blocks until the user closes it.
pub fn show_message_error_ok<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    window_title: P1,
    main_instruction: P2,
    message: P3,
) -> Result<(), XDialogError> {
    show_message_internal(XDialogIcon::Error, window_title, main_instruction, message, vec!["OK".to_string()])?;
    Ok(())
}

/// Shows a message box with OK/Cancel buttons and blocks until the user closes it.
/// Returns `true` if the OK button was pressed, `false` if the Cancel button was pressed or the dialog was closed.
pub fn show_message_ok_cancel<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    window_title: P1,
    main_instruction: P2,
    message: P3,
    icon: XDialogIcon,
) -> Result<bool, XDialogError> {
    let result = show_message_internal(icon, window_title, main_instruction, message, vec!["Cancel".to_string(), "OK".to_string()])?;
    Ok(result == XDialogResult::ButtonPressed(1))
}

/// Shows a message box with Yes/No buttons and blocks until the user closes it.
/// Returns `true` if the Yes button was pressed, `false` if the No button was pressed or the dialog was closed.
pub fn show_message_yes_no<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    window_title: P1,
    main_instruction: P2,
    message: P3,
    icon: XDialogIcon,
) -> Result<bool, XDialogError> {
    let result = show_message_internal(icon, window_title, main_instruction, message, vec!["No".to_string(), "Yes".to_string()])?;
    Ok(result == XDialogResult::ButtonPressed(1))
}

/// Shows a message box with Retry/Cancel buttons and blocks until the user closes it.
/// Returns `true` if the Retry button was pressed, `false` if the Cancel button was pressed or the dialog was closed.
pub fn show_message_retry_cancel<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    window_title: P1,
    main_instruction: P2,
    message: P3,
    icon: XDialogIcon,
) -> Result<bool, XDialogError> {
    let result = show_message_internal(icon, window_title, main_instruction, message, vec!["Cancel".to_string(), "Retry".to_string()])?;
    Ok(result == XDialogResult::ButtonPressed(1))
}

fn show_message_internal<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    icon: XDialogIcon,
    window_title: P1,
    main_instruction: P2,
    message: P3,
    buttons: Vec<String>,
) -> Result<XDialogResult, XDialogError> {
    let data = XDialogOptions {
        title: window_title.as_ref().to_string(),
        main_instruction: main_instruction.as_ref().to_string(),
        message: message.as_ref().to_string(),
        icon,
        buttons,
    };
    show_message(data)
}

/// Shows a message box with the specified options and blocks until the user closes it.
pub fn show_message(info: XDialogOptions) -> Result<XDialogResult, XDialogError> {
    if get_silent() {
        return Ok(XDialogResult::SilentMode);
    }

    let id = get_next_id();
    send_request(DialogMessageRequest::ShowMessageWindow(id, info))?;
    loop {
        if let Some(result) = get_result(id) {
            return Ok(result);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
