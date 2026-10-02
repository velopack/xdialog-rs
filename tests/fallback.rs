//! Windows: `Auto` falls back to Win32 TaskDialog when the drawn (Fluent) backend can't show a
//! dialog, and stays on TaskDialog for the rest of the session; an explicit `Fluent` reports the
//! failure instead. The drawn failure is forced with `XDIALOG_TEST_FAIL_DRAWN`, and TaskDialogs
//! are stubbed (`XDIALOG_TEST_STUB_TASKDIALOG`: no window; a message box answers at once with its
//! default button). `harness = false`: one winit loop per process, so the explicit-`Fluent` case
//! runs in a child process (`<exe> explicit`).

#[cfg(windows)]
fn main() {
    use std::time::Duration;

    // A hung loop must fail the test, not stall CI.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(30));
        eprintln!("fallback: timed out");
        std::process::exit(1);
    });
    std::env::set_var("XDIALOG_TEST_FAIL_DRAWN", "1");
    std::env::set_var("XDIALOG_TEST_STUB_TASKDIALOG", "1");
    std::env::set_var("XDIALOG_TEST_NO_ACTIVATE", "1");
    std::env::set_var("XDIALOG_TEST_POS", "offscreen");
    std::env::remove_var("XDIALOG_BACKEND");

    if std::env::args().nth(1).as_deref() == Some("explicit") {
        explicit_fluent_reports_the_failure();
        return;
    }
    auto_falls_back_to_task_dialog();
    let status = std::process::Command::new(std::env::current_exe().unwrap()).arg("explicit").status().unwrap();
    assert!(status.success(), "explicit Fluent: {status}");
    println!("fallback: ok");
}

#[cfg(not(windows))]
fn main() {
    println!("fallback: skipped (the TaskDialog fallback is Windows only)");
}

#[cfg(windows)]
fn options(title: &str) -> xdialog::XDialogOptions {
    xdialog::XDialogOptions { title: title.into(),
                              main_instruction: "Fallback".into(),
                              message: "The drawn backend failed.".into(),
                              icon: xdialog::XDialogIcon::Information,
                              icon_source: None,
                              buttons: vec!["No".into(), "Yes".into()] }
}

/// `Auto`: the request lands on the (stubbed) TaskDialog, which answers with its default button;
/// so does every later dialog, message or progress, even once the drawn backend would work again.
#[cfg(windows)]
fn auto_falls_back_to_task_dialog() {
    use std::time::Duration;
    use xdialog::*;

    XDialogBuilder::new().run(|| {
                             // A drawn dialog never answers by itself: the timeout would elapse.
                             let first = show_message(options("first")).wait_timeout(Duration::from_secs(10)).unwrap();
                             assert_eq!(first, XDialogResult::ButtonPressed(1), "the first dialog went to TaskDialog");
                             std::env::remove_var("XDIALOG_TEST_FAIL_DRAWN");
                             let second = show_message(options("second")).wait_timeout(Duration::from_secs(10)).unwrap();
                             assert_eq!(second, XDialogResult::ButtonPressed(1), "the session stays on TaskDialog");
                             let progress = show_progress_ex(XDialogOptions { buttons: vec!["Hide".into()], ..options("progress") }).unwrap();
                             progress.set_value(0.5).unwrap();
                             progress.close().unwrap();
                         });
}

/// An explicit `Fluent` has no fallback: the call fails with the drawn backend's error.
#[cfg(windows)]
fn explicit_fluent_reports_the_failure() {
    use xdialog::*;

    XDialogBuilder::new().with_backend(XDialogBackend::Fluent).run(|| {
                                                                  let result = show_message(options("explicit")).wait();
                                                                  assert!(matches!(result, Err(XDialogError::SystemError(_))), "{result:?}");
                                                                  let progress = show_progress("explicit", "Fallback", "Working", XDialogIcon::None);
                                                                  assert!(matches!(progress, Err(XDialogError::SystemError(_))), "{:?}", progress.err());
                                                              });
}
