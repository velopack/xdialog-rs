//! Linux without `builtin-winit` (e.g. `--no-default-features --features winit-host`):
//! `XDialogBuilder` has no backend, so dialog functions fail with `NoBackendAvailable` and `main`
//! still runs.
#![cfg(all(target_os = "linux", not(feature = "builtin-winit")))]

use xdialog::*;

#[test]
#[ntest::timeout(5000)]
fn builder_without_builtin_winit_has_no_backend() {
    let r = XDialogBuilder::new().run_result(run);
    r.unwrap();
}

fn run() -> Result<(), String> {
    match show_message_info_ok("t", "m", "b") {
        Err(XDialogError::NoBackendAvailable) => {}
        other => return Err(format!("show_message_info_ok: {other:?}")),
    }
    match show_progress("t", "m", "b", XDialogIcon::Information) {
        Err(XDialogError::NoBackendAvailable) => {}
        Ok(_) => return Err("show_progress: Ok".into()),
        Err(e) => return Err(format!("show_progress: {e:?}")),
    }
    Ok(())
}
