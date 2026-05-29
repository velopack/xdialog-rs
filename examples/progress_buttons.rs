//! Demonstrates progress dialogs with custom buttons.
//!
//! - `show_progress_with_callback` adds a "Cancel" button and reacts to clicks, keeping the dialog
//!   open to show a "Cancelling..." message until the work loop tears it down.
//! - `show_progress_ex` adds a "Hide" button with no callback (clicking it simply closes the
//!   dialog). On Windows this relabels the button that is always present on a progress dialog.
//! - `show_progress` shows the original button-less dialog (unchanged behavior).
//!
//! Run with: cargo run --example progress_buttons

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use xdialog::*;

fn main() {
    XDialogBuilder::new().with_backend(XDialogBackend::Skia).run(run);
}

fn run() {
    // 1. A cancellable operation. The "Cancel" button is shown on every platform, and the callback
    //    is notified when it is clicked.
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let progress = show_progress_with_callback(
        XDialogOptions {
            title: "Downloading".to_string(),
            main_instruction: "Downloading updates".to_string(),
            message: "Starting...".to_string(),
            icon: XDialogIcon::Information,
            buttons: vec!["Cancel".to_string()],
        },
        move |button_index, proxy| {
            println!("Progress button {} clicked -> cancelling", button_index);
            flag.store(true, Ordering::SeqCst);
            let _ = proxy.set_text("Cancelling, please wait...");
            true // keep the dialog open until we tear it down ourselves
        },
    )
    .unwrap();

    for i in 0..=100 {
        if cancelled.load(Ordering::SeqCst) {
            println!("Operation cancelled by user");
            std::thread::sleep(Duration::from_secs(1)); // pretend cleanup
            break;
        }
        progress.set_value(i as f32 / 100.0).unwrap();
        progress.set_text(format!("Downloaded {}%", i)).unwrap();
        std::thread::sleep(Duration::from_millis(60));
    }
    progress.close().unwrap();

    // 2. A button-only dialog with no callback. Clicking the button simply closes the dialog.
    let hide = show_progress_ex(XDialogOptions {
        title: "Working".to_string(),
        main_instruction: "Background task running".to_string(),
        message: "Click Hide to dismiss this window.".to_string(),
        icon: XDialogIcon::Information,
        buttons: vec!["Hide".to_string()],
    })
    .unwrap();
    hide.set_indeterminate().unwrap();
    std::thread::sleep(Duration::from_secs(3));
    hide.close().unwrap();

    // 3. The original button-less progress dialog (unchanged behavior).
    let plain = show_progress("Finishing", "Almost done", "No buttons here.", XDialogIcon::None).unwrap();
    plain.set_value(0.5).unwrap();
    std::thread::sleep(Duration::from_secs(2));
    plain.close().unwrap();
}
