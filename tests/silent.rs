//! Silent mode: no dialog is shown and no backend is needed.

use xdialog::*;

#[test]
fn message_dialogs_work_in_silent_mode() {
    set_silent_mode(true);
    show_message_info_ok("Silent", "Test", "Body").unwrap();
    show_message_warn_ok("Silent", "Test", "Body").unwrap();
    show_message_error_ok("Silent", "Test", "Body").unwrap();
    assert!(!show_message_ok_cancel("Silent", "Test", "Body", XDialogIcon::Information).unwrap());
    assert!(!show_message_yes_no("Silent", "Test", "Body", XDialogIcon::Warning).unwrap());
    assert!(!show_message_retry_cancel("Silent", "Test", "Body", XDialogIcon::Error).unwrap());
}

#[test]
fn progress_proxy_works_in_silent_mode() {
    set_silent_mode(true);
    let progress = show_progress("Silent Test", "Testing silent mode", "No dialog should appear", XDialogIcon::Information).unwrap();
    progress.set_value(0.5).unwrap();
    progress.set_text("Updating...").unwrap();
    progress.set_indeterminate().unwrap();
    progress.close().unwrap();
    progress.close().unwrap(); // double close should also be fine
    // Drop will call close() again
}

#[test]
fn show_message_returns_silent_mode() {
    set_silent_mode(true);
    let dialog = show_message(XDialogOptions { title: "Silent".into(), buttons: vec!["OK".into()], ..Default::default() });
    assert!(matches!(dialog.try_result(), Some(Ok(XDialogResult::SilentMode))));
    assert!(matches!(dialog.wait_timeout(std::time::Duration::from_millis(1)), Ok(XDialogResult::SilentMode)));
}

/// Without a backend (none is initialized here), only silent mode lets this return `Ok`; the
/// proxy's calls and its drop are no-ops.
#[test]
fn progress_with_callback_works_in_silent_mode() {
    set_silent_mode(true);
    let options = XDialogOptions { title: "Silent".into(), buttons: vec!["Cancel".into()], ..Default::default() };
    let progress = show_progress_with_callback(options, |_, _| true).unwrap();
    progress.set_value(0.5).unwrap();
    progress.close().unwrap();
    drop(progress);
}
