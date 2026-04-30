#![cfg(not(target_os = "macos"))]

use std::time::{Duration, Instant};
use xdialog::*;

#[test]
#[ntest::timeout(10000)]
fn message_dialog_respects_timeout() {
    XDialogBuilder::new().run(run);
}

fn run() {
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
