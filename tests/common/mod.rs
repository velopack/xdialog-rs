//! Shared setup of the `harness = false` tests that open real windows.

use std::time::Duration;

/// Keeps test windows from taking focus (unless the variables are already set, so a caller can
/// override them) and fails the process after `timeout`: a hung loop must not stall CI.
pub fn harness(name: &'static str, timeout: Duration) {
    for (var, value) in [("XDIALOG_TEST_NO_ACTIVATE", "1"), ("XDIALOG_TEST_POS", "offscreen")] {
        if std::env::var_os(var).is_none() {
            std::env::set_var(var, value);
        }
    }
    std::thread::spawn(move || {
        std::thread::sleep(timeout);
        eprintln!("{name}: timed out");
        std::process::exit(1);
    });
}
