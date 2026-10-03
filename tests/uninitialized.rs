//! Dialog calls before any backend runs (its own test binary, so no request handler is ever
//! installed): every API reports `NotInitialized`.

use xdialog::*;

#[test]
fn dialogs_without_a_backend_are_not_initialized() {
    let dialog = show_message(XDialogOptions::default());
    assert!(matches!(dialog.try_result(), Some(Err(XDialogError::NotInitialized))));
    assert!(matches!(dialog.wait(), Err(XDialogError::NotInitialized)));
    drop(dialog); // doesn't panic

    assert!(matches!(show_progress("t", "m", "b", XDialogIcon::Information), Err(XDialogError::NotInitialized)));
    assert!(matches!(show_message_info_ok("t", "m", "b"), Err(XDialogError::NotInitialized)));
    assert!(matches!(show_message_yes_no("t", "m", "b", XDialogIcon::None), Err(XDialogError::NotInitialized)));
}
