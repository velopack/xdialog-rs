//! Tests that xdialog gracefully returns NoBackendAvailable on headless Linux.
//!
//! This is a standalone binary (not part of the main crate's test suite) so it
//! avoids pulling in dev-dependencies like xcap/pipewire. Build and run with:
//!
//!   cargo run --manifest-path tests/headless/Cargo.toml
//!
//! Or via cross-rs for multi-target testing:
//!
//!   cross run --manifest-path tests/headless/Cargo.toml --target aarch64-unknown-linux-gnu

fn main() {
    #[cfg(target_os = "linux")]
    {
        // Clear display variables to ensure headless environment
        std::env::remove_var("DISPLAY");
        std::env::remove_var("WAYLAND_DISPLAY");

        xdialog::XDialogBuilder::new().run(run_headless_tests);
        eprintln!("All headless tests passed");
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
        "Expected NoBackendAvailable from show_progress"
    );
    eprintln!("PASS: show_progress returned NoBackendAvailable");
}
