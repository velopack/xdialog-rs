//! On Linux without a display server, every dialog function must return `NoBackendAvailable`
//! while the program keeps running. `harness = false`; CI also builds it static for musl, so it
//! loads no userland libraries:
//!
//!   cargo test --release --target x86_64-unknown-linux-musl --test headless

fn main() {
    #[cfg(target_os = "linux")]
    {
        std::env::remove_var("DISPLAY");
        std::env::remove_var("WAYLAND_DISPLAY");
        xdialog::XDialogBuilder::new().run(|| {
            use xdialog::*;
            let results = [show_message_info_ok("Test", "Heading", "Body").map(drop),
                           show_message_yes_no("Test", "Heading", "Body", XDialogIcon::Information).map(drop),
                           show_progress("Test", "Heading", "Body", XDialogIcon::Information).map(drop)];
            for r in results {
                assert!(matches!(r, Err(XDialogError::NoBackendAvailable)), "expected NoBackendAvailable, got {r:?}");
            }
        });
        eprintln!("All headless tests passed");
    }

    #[cfg(not(target_os = "linux"))]
    eprintln!("Skipping headless test (Linux only)");
}
