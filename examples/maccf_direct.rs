/// This example demonstrates the maccf-direct feature, which allows calling
/// xdialog functions directly without XDialogBuilder or run_loop.
///
/// Run with: cargo run --example maccf_direct --features maccf-direct
use std::thread::sleep;
use std::time::Duration;

fn main() {
    xdialog::init_maccf_direct();
    // No XDialogBuilder needed - just call show functions directly.
    let result = xdialog::show_message(
        xdialog::XDialogOptions {
            title: "My App".to_string(),
            main_instruction: "Hello from maccf-direct!".to_string(),
            message: "This dialog was shown without any event loop.\nPick an option:".to_string(),
            icon: xdialog::XDialogIcon::Information,
            buttons: vec!["Save".to_string(), "Discard".to_string(), "Cancel".to_string()],
        },
        None,
    )
    .unwrap();

    let msg = match result {
        xdialog::XDialogResult::ButtonPressed(0) => "You chose Save.",
        xdialog::XDialogResult::ButtonPressed(1) => "You chose Discard.",
        xdialog::XDialogResult::ButtonPressed(2) => "You chose Cancel.",
        _ => "Dialog was closed.",
    };

    xdialog::show_message_info_ok("My App", "Result", msg).unwrap();

    // Progress dialogs are supported too. CFUserNotification has no native progress bar, so
    // progress is drawn as an animated unicode text bar below the body text.
    let progress = xdialog::show_progress(
        "My App",
        "Working...",
        "Downloading files",
        xdialog::XDialogIcon::Information,
    )
    .unwrap();

    // Determinate: a filled bar with a trailing percentage.
    for i in 0..=10 {
        progress.set_value(i as f32 / 10.0).unwrap();
        progress.set_text(format!("Downloading files ({}/10)", i)).unwrap();
        sleep(Duration::from_millis(400));
    }

    // Indeterminate: a segment bounces across the bar until we close it.
    progress.set_indeterminate().unwrap();
    progress.set_text("Finishing up...").unwrap();
    sleep(Duration::from_secs(3));

    progress.close().unwrap();
}
