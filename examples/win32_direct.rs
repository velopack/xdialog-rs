/// This example demonstrates the win32-direct feature, which allows calling
/// xdialog functions directly without XDialogBuilder or run_loop.
///
/// Run with: cargo run --example win32_direct --no-default-features --features win32-direct
fn main() {
    // No XDialogBuilder needed - just call show functions directly.
    let yes = xdialog::show_message_yes_no(
        "My App",
        "Hello from win32-direct!",
        "This dialog was shown without any initialization.\nWould you like to see a progress bar?",
        xdialog::XDialogIcon::Information,
    )
    .unwrap();

    if !yes {
        return;
    }

    let progress = xdialog::show_progress(
        "My App",
        "Doing some work",
        "Starting...",
        xdialog::XDialogIcon::Information,
    )
    .unwrap();

    for i in 1..=5 {
        progress.set_value(i as f32 / 5.0).unwrap();
        progress.set_text(&format!("Step {i} of 5...")).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    progress.close().unwrap();

    xdialog::show_message_info_ok("My App", "All done!", "The work completed successfully.").unwrap();
}
