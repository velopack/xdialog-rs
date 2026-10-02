//! Dialog life cycle through `XDialogBuilder::run` with the default backend: a message box closed
//! by its timeout, a progress dialog closed several times. Shows real windows. `harness = false`:
//! macOS wants the UI on the main thread.

mod common;

use std::time::{Duration, Instant};
use xdialog::*;

fn main() {
    common::harness("lifecycle", Duration::from_secs(20));
    XDialogBuilder::new().run(|| {
                             message_times_out(Duration::from_secs(1));
                             progress_close_twice();
                         });
    println!("lifecycle: ok");
}

fn message_times_out(timeout: Duration) {
    let start = Instant::now();
    let options = XDialogOptions { title: "Timeout Test".to_string(),
                                   main_instruction: "Testing timeout".to_string(),
                                   message: format!("This dialog should auto-close after {timeout:?}"),
                                   icon: XDialogIcon::Information,
                                   buttons: vec!["OK".to_string()],
                                   ..Default::default() };
    let result = show_message(options).wait_timeout(timeout).unwrap();
    assert_eq!(result, XDialogResult::TimeoutElapsed);
    assert!(start.elapsed() >= timeout, "dialog closed too early: {:?}", start.elapsed());
}

fn progress_close_twice() {
    let progress = show_progress("Close Twice Test", "Testing double close", "Calling close multiple times should not error", XDialogIcon::Information).unwrap();
    progress.close().unwrap();
    progress.close().unwrap();
    progress.close().unwrap();
    // Drop closes a fourth time.
    drop(progress);
    // The extra closes leave the runtime working: the next dialog opens and times out normally.
    message_times_out(Duration::from_millis(500));
}
