//! Tests that xdialog gracefully returns NoBackendAvailable on headless Linux.
//!
//! Run with: XDIALOG_HEADLESS_TEST=1 cargo test --test headless_linux

fn main() {
    #[cfg(target_os = "linux")]
    {
        if std::env::var("XDIALOG_HEADLESS_TEST").is_err() {
            eprintln!(
                "Skipping headless test (set XDIALOG_HEADLESS_TEST=1 to run)"
            );
            return;
        }

        // Clear display variables to ensure headless environment
        std::env::remove_var("DISPLAY");
        std::env::remove_var("WAYLAND_DISPLAY");

        xdialog::XDialogBuilder::new().run(run_headless_tests);
    }

    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("Skipping headless test (Linux only)");
    }
}

#[cfg(target_os = "linux")]
fn run_headless_tests() {
    test_message_no_backend();
    test_progress_no_backend();
    eprintln!("All headless tests passed");
}

#[cfg(target_os = "linux")]
fn test_message_no_backend() {
    use xdialog::*;

    let result = show_message_info_ok("Test", "Heading", "Body");
    assert!(
        matches!(result, Err(XDialogError::NoBackendAvailable)),
        "Expected NoBackendAvailable, got: {:?}",
        result
    );
    eprintln!("PASS: show_message_info_ok returned NoBackendAvailable");

    let result = show_message_yes_no("Test", "Heading", "Body", XDialogIcon::Information);
    assert!(
        matches!(result, Err(XDialogError::NoBackendAvailable)),
        "Expected NoBackendAvailable, got: {:?}",
        result
    );
    eprintln!("PASS: show_message_yes_no returned NoBackendAvailable");
}

#[cfg(target_os = "linux")]
fn test_progress_no_backend() {
    use xdialog::*;

    let result = show_progress("Test", "Heading", "Body", XDialogIcon::Information);
    assert!(
        matches!(result, Err(XDialogError::NoBackendAvailable)),
        "Expected NoBackendAvailable, got: {:?}",
        result
    );
    eprintln!("PASS: show_progress returned NoBackendAvailable");
}
