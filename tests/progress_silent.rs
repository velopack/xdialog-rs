use xdialog::*;

#[test]
#[ntest::timeout(2000)]
fn progress_proxy_works_in_silent_mode() {
    set_silent_mode(true);

    let progress = show_progress(
        "Silent Test",
        "Testing silent mode",
        "No dialog should appear",
        XDialogIcon::Information,
    )
    .unwrap();

    progress.set_value(0.5).unwrap();
    progress.set_text("Updating...").unwrap();
    progress.set_indeterminate().unwrap();
    progress.close().unwrap();
    progress.close().unwrap(); // double close should also be fine
    // Drop will call close() again
}
