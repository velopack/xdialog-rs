use xdialog::*;

#[test]
#[ntest::timeout(2000)]
fn message_dialogs_work_in_silent_mode() {
    set_silent_mode(true);

    show_message_info_ok("Silent", "Test", "Body").unwrap();
    show_message_warn_ok("Silent", "Test", "Body").unwrap();
    show_message_error_ok("Silent", "Test", "Body").unwrap();

    let ok_cancel = show_message_ok_cancel("Silent", "Test", "Body", XDialogIcon::Information).unwrap();
    assert!(!ok_cancel);

    let yes_no = show_message_yes_no("Silent", "Test", "Body", XDialogIcon::Warning).unwrap();
    assert!(!yes_no);

    let retry_cancel = show_message_retry_cancel("Silent", "Test", "Body", XDialogIcon::Error).unwrap();
    assert!(!retry_cancel);
}
