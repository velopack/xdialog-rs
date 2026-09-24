//! The `linux-direct` feature: show the Linux-style (egui) dialogs without an `XDialogBuilder` or
//! an event loop of your own. xdialog starts its own UI thread on the first dialog.
//!
//! Run with: `cargo run --example linux_direct --features linux-direct`
//! (Linux and Windows; on Windows it shows the Linux look for development.)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use xdialog::*;

fn main() {
    // Cheap: only installs a request handler. No thread or event loop exists yet.
    init_linux_direct(XDialogTheme::SystemDefault);

    // Dialog functions work from `main` (or any other thread).
    let yes = show_message_yes_no("My App",
                                  "Hello from linux-direct!",
                                  "This dialog was shown without an XDialogBuilder.\nWould you like to see a progress bar?",
                                  XDialogIcon::Information).unwrap();
    if !yes {
        return;
    }

    // A progress dialog with a Cancel button. The callback runs on the xdialog UI thread, so it
    // must not call a blocking function like `show_message` (that returns
    // `XDialogError::BlockingCallOnUiThread`); it just sets a flag.
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let progress = show_progress_with_callback(XDialogOptions { title: "My App".into(),
                                                                main_instruction: "Doing some work".into(),
                                                                message: "Starting...".into(),
                                                                icon: XDialogIcon::Information,
                                                                buttons: vec!["Cancel".into()] },
                                               move |_, proxy| {
                                                   flag.store(true, Ordering::SeqCst);
                                                   let _ = proxy.set_text("Cancelling...");
                                                   true
                                               }).unwrap();

    for i in 1..=5 {
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
        progress.set_value(i as f32 / 5.0).unwrap();
        progress.set_text(format!("Step {i} of 5...")).unwrap();
        std::thread::sleep(Duration::from_secs(1));
    }
    progress.close().unwrap();

    if cancelled.load(Ordering::SeqCst) {
        show_message_warn_ok("My App", "Cancelled", "The work was cancelled.").unwrap();
    } else {
        show_message_info_ok("My App", "All done!", "The work completed successfully.").unwrap();
    }
}
