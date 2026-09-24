//! linux-direct mode: `init_linux_direct`, then dialogs from test
//! threads with no `XDialogBuilder`.
//!
//! Windows are created without activation (`XDIALOG_TEST_NO_ACTIVATE`) left of the virtual desktop
//! (`XDIALOG_TEST_POS=offscreen`), so they never take focus or appear on a monitor. With
//! `_test-hooks` the button-callback tests click through `xdialog::__test::inject` (never real
//! input):
//!
//! `cargo test --test egui_direct --features linux-direct,_test-hooks`

use std::sync::Once;
use std::time::{Duration, Instant};

use xdialog::*;

fn setup() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
                     std::env::set_var("XDIALOG_TEST_NO_ACTIVATE", "1");
                     std::env::set_var("XDIALOG_TEST_POS", "offscreen");
                     init_linux_direct(XDialogTheme::Light);
                     // A second call is ignored (with a warning) and must not break the first.
                     init_linux_direct(XDialogTheme::Dark);
                 });
}

fn opts(title: &str, buttons: &[&str]) -> XDialogOptions {
    XDialogOptions { title: title.to_string(),
                     main_instruction: "linux-direct test".to_string(),
                     message: "Shown by the egui_direct integration test.".to_string(),
                     icon: XDialogIcon::Information,
                     buttons: buttons.iter().map(|s| s.to_string()).collect() }
}

#[test]
#[ntest::timeout(30000)]
fn progress_show_update_close() {
    setup();
    let progress = show_progress("xdialog direct progress", "Working", "Starting...", XDialogIcon::Information).unwrap();
    progress.set_value(0.25).unwrap();
    progress.set_text("Step 1").unwrap();
    progress.set_indeterminate().unwrap();
    progress.set_value(1.0).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    progress.close().unwrap();
    // Closing twice (and the drop) is harmless.
    progress.close().unwrap();
}

#[test]
#[ntest::timeout(30000)]
fn message_timeout() {
    setup();
    let timeout = Duration::from_millis(700);
    let start = Instant::now();
    let r = show_message(opts("xdialog direct timeout", &["OK"]), Some(timeout)).unwrap();
    assert_eq!(r, XDialogResult::TimeoutElapsed);
    assert!(start.elapsed() >= timeout, "closed too early: {:?}", start.elapsed());
}

#[test]
#[ntest::timeout(30000)]
fn many_threads_at_once() {
    setup();
    let workers: Vec<_> = (0..4).map(|i| {
                                    std::thread::spawn(move || {
                                        let t = format!("xdialog direct worker {i}");
                                        show_message(opts(&t, &["OK"]), Some(Duration::from_millis(300))).unwrap()
                                    })
                                })
                                .collect();
    for w in workers {
        assert_eq!(w.join().unwrap(), XDialogResult::TimeoutElapsed);
    }
}

#[cfg(feature = "_test-hooks")]
mod hooks {
    use std::sync::mpsc::channel;

    use xdialog::__test::{inject, live_dialogs, HostEvent, LiveDialog, MouseButton};

    use super::*;

    fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(t0.elapsed() < Duration::from_secs(10), "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// None of our windows is the foreground window (they are created without activation).
    fn assert_not_foreground(d: &LiveDialog) {
        #[cfg(windows)]
        {
            use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
            // SAFETY: plain query.
            let fg = unsafe { GetForegroundWindow() }.0 as isize;
            assert_ne!(fg, d.raw_window, "a dialog window became the foreground window");
        }
        let _ = d;
    }

    fn live(title: &str) -> Option<LiveDialog> {
        live_dialogs().into_iter().find(|d| d.title == title && d.frames > 0 && !d.button_rects_px.is_empty())
    }

    /// Click button `index` of a live dialog through the injection hook.
    fn click(d: &LiveDialog, index: usize) {
        let (x, y) = d.button_centre(index).expect("button");
        inject(d.id, HostEvent::CursorMoved { x, y });
        inject(d.id, HostEvent::MouseButton { button: MouseButton::Primary, pressed: true });
        inject(d.id, HostEvent::MouseButton { button: MouseButton::Primary, pressed: false });
    }

    /// A progress callback runs on the xdialog UI thread: `show_message` there fails with
    /// `BlockingCallOnUiThread` instead of deadlocking, and `show_progress` returns at once; its
    /// window is created on the next loop iteration.
    #[test]
    #[ntest::timeout(30000)]
    fn callback_guards() {
        setup();
        let (tx, rx) = channel();
        let title = "xdialog direct callback";
        let nested_title = "xdialog direct nested";
        let progress = show_progress_with_callback(opts(title, &["Cancel"]), move |i, proxy| {
                           let _ = proxy.set_text("Cancelling...");
                           let blocked = show_message(opts("never shown", &["OK"]), None);
                           let t0 = Instant::now();
                           let nested = show_progress_ex(opts(nested_title, &["Hide"]));
                           let nested_ms = t0.elapsed();
                           let _ = tx.send((i, blocked, nested_ms, nested.is_ok()));
                           // Keep the nested dialog open until the test closes it.
                           std::mem::forget(nested);
                           true
                       }).unwrap();

        let d = wait_for("the progress dialog", || live(title));
        assert_not_foreground(&d);
        click(&d, 0);
        let (i, blocked, nested_ms, nested_ok) = rx.recv_timeout(Duration::from_secs(10)).expect("callback did not run");
        assert_eq!(i, 0);
        assert!(matches!(blocked, Err(XDialogError::BlockingCallOnUiThread)), "{blocked:?}");
        assert!(nested_ok);
        assert!(nested_ms < Duration::from_millis(500), "show_progress blocked on the UI thread: {nested_ms:?}");

        // The nested dialog appears, and the first one stayed open (callback returned true).
        let nested = wait_for("the nested dialog", || live(nested_title));
        assert!(live(title).is_some());
        assert_not_foreground(&nested);

        // Clicking the nested dialog's button (no callback) closes it.
        click(&nested, 0);
        wait_for("the nested dialog to close", || live_dialogs().iter().all(|d| d.id != nested.id).then_some(()));

        progress.close().unwrap();
        wait_for("the progress dialog to close", || live_dialogs().iter().all(|x| x.id != d.id).then_some(()));
    }

    /// A panicking callback closes its dialog; the UI thread keeps serving requests.
    #[test]
    #[ntest::timeout(30000)]
    fn panicking_callback_keeps_loop() {
        setup();
        let title = "xdialog direct panic";
        let _progress = show_progress_with_callback(opts(title, &["Boom"]), |_, _| panic!("test panic in a progress callback")).unwrap();
        let d = wait_for("the panicking dialog", || live(title));
        click(&d, 0);
        wait_for("the panicking dialog to close", || live_dialogs().iter().all(|x| x.id != d.id).then_some(()));
        let r = show_message(opts("xdialog direct after panic", &["OK"]), Some(Duration::from_millis(200))).unwrap();
        assert_eq!(r, XDialogResult::TimeoutElapsed);
    }
}
