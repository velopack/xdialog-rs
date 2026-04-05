use std::time::{Duration, Instant};
use xdialog::*;

fn main() {
    #[cfg(target_os = "macos")]
    {
        XDialogBuilder::new().run(run);
    }
}

#[cfg(target_os = "macos")]
fn run() {
    test_message_timeout();
    test_progress_close_twice();
}

#[cfg(target_os = "macos")]
fn test_message_timeout() {
    let timeout = Duration::from_secs(1);
    let start = Instant::now();

    let result = show_message(
        XDialogOptions {
            title: "Timeout Test".to_string(),
            main_instruction: "Testing timeout".to_string(),
            message: "This dialog should auto-close after 1 second".to_string(),
            icon: XDialogIcon::Information,
            buttons: vec!["OK".to_string()],
        },
        Some(timeout),
    )
    .unwrap();

    let elapsed = start.elapsed();
    assert_eq!(result, XDialogResult::TimeoutElapsed);
    assert!(elapsed >= timeout, "dialog closed too early: {:?}", elapsed);
}

#[cfg(target_os = "macos")]
fn test_progress_close_twice() {
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
