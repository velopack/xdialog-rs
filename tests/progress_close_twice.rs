#![cfg(not(target_os = "macos"))]

use std::time::Duration;
use xdialog::*;

#[test]
#[ntest::timeout(2000)]
fn progress_close_can_be_called_multiple_times() {
    XDialogBuilder::new().run(run);
}

fn run() {
    let progress = show_progress(
        "Close Twice Test",
        "Testing double close",
        "Calling close multiple times should not error",
        XDialogIcon::Information,
    )
    .unwrap();

    std::thread::sleep(Duration::from_millis(200));

    progress.close().unwrap();
    progress.close().unwrap();
    progress.close().unwrap();
    // Drop will call close() a fourth time
}
