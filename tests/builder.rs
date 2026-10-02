//! `XDialogBuilder::run*` without a dialog: `main`'s value comes back, a later builder runs `main`
//! without a backend, dialog calls after the loop ended get `NoBackendAvailable`, and a panic in
//! `main` reaches the caller once the loop stopped (a lost `ExitEventLoop` would hang instead; the
//! first builder of a process runs in a child, `<exe> panic`). `harness = false`: macOS wants the
//! UI on the main thread, and one winit loop per process.

use std::panic::catch_unwind;
use std::time::Duration;
use xdialog::*;

fn main() {
    // A hung loop must fail the test, not stall CI.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(30));
        eprintln!("builder: timed out");
        std::process::exit(1);
    });
    std::env::set_var("XDIALOG_TEST_NO_ACTIVATE", "1");
    std::env::set_var("XDIALOG_TEST_POS", "offscreen");
    std::env::remove_var("XDIALOG_BACKEND");

    if std::env::args().nth(1).as_deref() == Some("panic") {
        XDialogBuilder::new().run(|| panic!("builder-panic"));
        return;
    }

    assert_eq!(XDialogBuilder::new().run_i32(|| 7), 7);
    assert_eq!(XDialogBuilder::new().run_result(|| Err::<(), _>("e")), Err("e"));
    assert!(catch_unwind(|| XDialogBuilder::new().run(|| panic!("in-process panic (expected)"))).is_err());
    let after = after_the_loop().try_result();
    assert!(matches!(after, Some(Err(XDialogError::NoBackendAvailable))), "{after:?}");

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(std::process::Command::new(std::env::current_exe().unwrap()).arg("panic").output());
    });
    let output = rx.recv_timeout(Duration::from_secs(10)).expect("a panic in main hung the builder").unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "panic child: {}", output.status);
    assert!(stderr.contains("builder-panic"), "panic child stderr: {stderr}");
    println!("builder: ok");
}

fn after_the_loop() -> MessageDialogProxy {
    show_message(XDialogOptions { title: "After".into(),
                                  main_instruction: "After the loop".into(),
                                  message: "Never shown.".into(),
                                  icon: XDialogIcon::Information,
                                  icon_source: None,
                                  buttons: vec!["OK".into()] })
}
