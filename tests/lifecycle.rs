//! Dialog life cycle through `XDialogBuilder::run` with the default backend: a message box closed
//! by its timeout, a progress dialog closed several times. Shows real windows. `harness = false`:
//! macOS wants the UI on the main thread.

use std::time::{Duration, Instant};
use xdialog::*;

fn main() {
    // A hung loop must fail the test, not stall CI.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(20));
        eprintln!("lifecycle: timed out");
        std::process::exit(1);
    });
    XDialogBuilder::new().run(|| {
                             message_timeout();
                             progress_close_twice();
                         });
    println!("lifecycle: ok");
}

fn message_timeout() {
    let timeout = Duration::from_secs(1);
    let start = Instant::now();
    let options = XDialogOptions { title: "Timeout Test".to_string(),
                                   main_instruction: "Testing timeout".to_string(),
                                   message: "This dialog should auto-close after 1 second".to_string(),
                                   icon: XDialogIcon::Information,
                                   buttons: vec!["OK".to_string()] };
    let result = show_message(options).wait_timeout(timeout).unwrap();
    assert_eq!(result, XDialogResult::TimeoutElapsed);
    assert!(start.elapsed() >= timeout, "dialog closed too early: {:?}", start.elapsed());
}

fn progress_close_twice() {
    let progress = show_progress("Close Twice Test", "Testing double close", "Calling close multiple times should not error", XDialogIcon::Information).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    progress.close().unwrap();
    progress.close().unwrap();
    progress.close().unwrap();
    // Drop will call close() a fourth time
}
