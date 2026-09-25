//! Host mode (`winit-host`): an `XDialogApp` wrapping the test's own handler, driven with
//! `pump_app_events`. Input is injected with the `_test-hooks` API; windows never activate and are
//! placed off every monitor. `harness = false`: winit wants its loop on the main thread.
//!
//! Skipped (passes) when no event loop can be created (Linux without a display server). On macOS
//! only the backend choice is checked (explicit AppKit refused, `Auto` accepted).

use std::time::Duration;

use xdialog::*;

fn main() {
    // Environment first: xdialog reads it when it creates windows.
    std::env::set_var("XDIALOG_TEST_NO_ACTIVATE", "1");
    std::env::set_var("XDIALOG_TEST_POS", "offscreen");
    // A hung loop must fail the test, not stall CI.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(120));
        eprintln!("winit_host: timed out");
        std::process::exit(1);
    });
    run();
    println!("winit_host: ok");
}

#[cfg(target_os = "macos")]
fn run() {
    use xdialog::host::winit::application::ApplicationHandler;
    use xdialog::host::winit::event::WindowEvent;
    use xdialog::host::winit::event_loop::ActiveEventLoop;
    use xdialog::host::winit::window::WindowId;

    struct Inner;
    impl ApplicationHandler for Inner {
        fn resumed(&mut self, _: &ActiveEventLoop) {}
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    // AppKit needs its own loop; `Auto` falls back to an egui look.
    let appkit = XDialogBuilder::new().with_backend(XDialogBackend::AppKit).into_host_app(Inner, || {});
    assert!(matches!(appkit, Err(XDialogError::NoBackendAvailable)));
    assert!(XDialogBuilder::new().into_host_app(Inner, || {}).is_ok());
}

#[cfg(not(target_os = "macos"))]
fn run() {
    use std::future::Future;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    use xdialog::__test::egui::{Event, Key, PointerButton, Pos2};
    use xdialog::host::winit::application::ApplicationHandler;
    use xdialog::host::winit::dpi::PhysicalSize;
    use xdialog::host::winit::event::{StartCause, WindowEvent};
    use xdialog::host::winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use xdialog::host::winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
    use xdialog::host::winit::window::{Window, WindowId};
    use xdialog::host::{LiveDialog, XDialogApp};

    /// The test's own handler: no xdialog code.
    #[derive(Default)]
    struct Inner {
        /// A hidden, inactive window of the host's own.
        window: Option<Window>,
        /// Its id (kept after `exiting` drops it: its late `Destroyed` is the host's own).
        own: Option<WindowId>,
        /// Events forwarded for `window`.
        own_events: usize,
        /// User events forwarded (the waker's included).
        user_events: usize,
        /// Set in the next `about_to_wait` (persists: never set again).
        set_flow: Option<ControlFlow>,
        /// `requested_resume` of every `ResumeTimeReached` seen.
        resumes: Vec<Instant>,
        quit: bool,
        exited: bool,
    }

    impl ApplicationHandler for Inner {
        fn new_events(&mut self, _: &ActiveEventLoop, cause: StartCause) {
            if let StartCause::ResumeTimeReached { requested_resume, .. } = cause {
                self.resumes.push(requested_resume);
            }
        }

        fn resumed(&mut self, el: &ActiveEventLoop) {
            if self.window.is_none() {
                let attrs = Window::default_attributes().with_visible(false).with_active(false).with_title("host");
                let window = el.create_window(attrs).unwrap();
                self.own = Some(window.id());
                self.window = Some(window);
            }
        }

        fn user_event(&mut self, _: &ActiveEventLoop, (): ()) {
            self.user_events += 1;
        }

        fn window_event(&mut self, _: &ActiveEventLoop, id: WindowId, _: WindowEvent) {
            if self.own == Some(id) {
                self.own_events += 1;
            } else {
                panic!("a dialog window's event reached the host app");
            }
        }

        fn about_to_wait(&mut self, el: &ActiveEventLoop) {
            if let Some(flow) = self.set_flow.take() {
                el.set_control_flow(flow);
            }
            if self.quit {
                el.exit();
            }
        }

        fn exiting(&mut self, _: &ActiveEventLoop) {
            self.exited = true;
            self.window = None;
        }
    }

    type App = XDialogApp<Inner>;

    let mut el = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => return println!("winit_host: skipped, no event loop: {e}"),
    };
    let proxy = el.create_proxy();
    let wakes = Arc::new(AtomicUsize::new(0));
    let w = wakes.clone();
    // A persistent host deadline far in the future: xdialog's comes first, then is undone.
    let far = Instant::now() + Duration::from_secs(3600);
    let inner = Inner { set_flow: Some(ControlFlow::WaitUntil(far)), ..Default::default() };
    // Windows: Fluent without the TaskDialog fallback (TaskDialogs take focus).
    let backend = if cfg!(windows) { XDialogBackend::Fluent } else { XDialogBackend::Auto };
    let mut app = XDialogBuilder::new().with_backend(backend)
                                       .into_host_app(inner, move || {
                                           w.fetch_add(1, Ordering::SeqCst);
                                           let _ = proxy.send_event(());
                                       })
                                       .expect("into_host_app");

    let message = |text: &str| {
        let options = XDialogOptions { title: "t".into(), message: text.into(), buttons: vec!["OK".into()], ..Default::default() };
        std::thread::spawn(move || show_message(options).wait())
    };

    // Pump for `ms`, or until `done`. Short timeouts: the host's `WaitUntil(far)` must not turn a
    // missing wake-up into an hour-long hang.
    let pump = |el: &mut EventLoop<()>, app: &mut App, ms: u64, done: &dyn Fn(&mut App) -> bool| {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end && !done(app) {
            el.pump_app_events(Some(Duration::from_millis(5)), app);
        }
    };
    let first_dialog = |app: &App| app.test_dialogs().into_iter().next();
    let wait_dialog = |el: &mut EventLoop<()>, app: &mut App| -> LiveDialog {
        pump(el, app, 10_000, &|app| !app.test_dialogs().is_empty());
        first_dialog(app).expect("a dialog opened")
    };
    let titled = |app: &App, title: &str| app.test_dialogs().into_iter().find(|d| d.title == title);
    let wait_titled = |el: &mut EventLoop<()>, app: &mut App, title: &str| -> LiveDialog {
        pump(el, app, 10_000, &|app| titled(app, title).is_some());
        titled(app, title).unwrap_or_else(|| panic!("dialog '{title}' opened"))
    };
    // Click button `index` (move, press, release).
    let click = |el: &mut EventLoop<()>, app: &mut App, d: &LiveDialog, index: usize| {
        let [x, y, w, h] = d.button_rects[index];
        let pos = Pos2::new(x + w / 2.0, y + h / 2.0);
        let button = |pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() };
        app.test_inject(d.id, Event::PointerMoved(pos));
        app.test_inject(d.id, button(true));
        pump(el, app, 50, &|_| false);
        app.test_inject(d.id, button(false));
    };
    let deadline = |app: &App| match app.test_control_flow() {
        Some(ControlFlow::WaitUntil(t)) => Some(t),
        _ => None,
    };

    // One handler per process.
    assert!(matches!(XDialogBuilder::new().into_host_app(Inner::default(), || {}), Err(XDialogError::SystemError(_))));

    // The host's own window events are forwarded; its flow is untouched while xdialog is idle.
    pump(&mut el, &mut app, 1_000, &|app| app.inner().window.is_some());
    let _ = app.inner().window.as_ref().expect("host window").request_inner_size(PhysicalSize::new(321, 123));
    pump(&mut el, &mut app, 1_000, &|app| app.inner().own_events > 0);
    if cfg!(windows) {
        assert!(app.inner().own_events > 0, "the host window's events reach the host app");
    }
    assert_eq!(app.test_control_flow(), Some(ControlFlow::WaitUntil(far)));

    // Event-loop thread: blocking calls fail, progress works and animates.
    assert!(matches!(show_message_info_ok("t", "b", "c"), Err(XDialogError::BlockingCallOnUiThread)));
    let progress = show_progress("t", "Host thread", "body", XDialogIcon::Information).unwrap();
    progress.set_value(0.25).unwrap();
    progress.set_text("Step 1").unwrap();
    progress.set_indeterminate().unwrap();
    let d = wait_dialog(&mut el, &mut app);
    assert_eq!(d.title, "t");
    pump(&mut el, &mut app, 400, &|_| false);
    let frames = first_dialog(&app).unwrap().frames - d.frames;
    assert!(frames >= 10, "indeterminate progress drew {frames} frames in 400 ms");

    // Control flow. A later host deadline gives way to xdialog's (an iteration with a due frame's
    // redraw pending has none: wait for one that has) ...
    pump(&mut el, &mut app, 1_000, &|app| deadline(app).is_some_and(|t| t < far));
    assert!(deadline(&app).is_some_and(|t| t < far), "an animating dialog wakes the loop before a later host deadline");
    assert!(app.inner().resumes.is_empty(), "the host app never sees xdialog's deadline as its own");
    // ... an earlier one (already due) is kept and fires as the host's ...
    let early = Instant::now();
    app.inner_mut().set_flow = Some(ControlFlow::WaitUntil(early));
    pump(&mut el, &mut app, 100, &|_| false);
    assert_eq!(app.test_control_flow(), Some(ControlFlow::WaitUntil(early)));
    assert!(app.inner().resumes.contains(&early), "the host's own deadline fires as its own");
    // ... and `Poll` is never overridden.
    app.inner_mut().set_flow = Some(ControlFlow::Poll);
    pump(&mut el, &mut app, 100, &|_| false);
    assert_eq!(app.test_control_flow(), Some(ControlFlow::Poll));
    app.inner_mut().set_flow = Some(ControlFlow::WaitUntil(far));
    pump(&mut el, &mut app, 20, &|_| false);
    app.inner_mut().resumes.clear();

    drop(progress);
    pump(&mut el, &mut app, 2_000, &|app| app.test_dialogs().is_empty());
    assert!(app.test_dialogs().is_empty(), "dropping the proxy closes the dialog");
    pump(&mut el, &mut app, 100, &|_| false);
    assert_eq!(app.test_control_flow(), Some(ControlFlow::WaitUntil(far)), "idle again: the host's flow is back");

    // Worker thread: the waker brings the dialog up, its event reaches the host app; click "Yes".
    let (wakes_before, user_before) = (wakes.load(Ordering::SeqCst), app.inner().user_events);
    let worker = std::thread::spawn(|| show_message_yes_no("t", "Question", "Yes or no?", XDialogIcon::Warning));
    let d = wait_dialog(&mut el, &mut app);
    assert!(wakes.load(Ordering::SeqCst) > wakes_before, "the request woke the loop");
    assert!(app.inner().user_events > user_before, "the wake-up is forwarded to the host app");
    click(&mut el, &mut app, &d, 1);
    pump(&mut el, &mut app, 2_000, &|_| worker.is_finished());
    assert!(worker.is_finished(), "the click answered the dialog");
    assert!(matches!(worker.join().unwrap(), Ok(true)));

    // Event-loop thread: `show_message` returns at once, waiting on it fails fast, and the answer
    // wakes the loop so the host's `try_result` sees it; it can also be polled as a future.
    let options = XDialogOptions { title: "host question".into(), buttons: vec!["No".into(), "Yes".into()], ..Default::default() };
    let mut question = show_message(options);
    assert!(question.try_result().is_none());
    assert!(matches!(question.wait(), Err(XDialogError::BlockingCallOnUiThread)));
    let d = wait_titled(&mut el, &mut app, "host question");
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(std::pin::Pin::new(&mut question).poll(&mut cx).is_pending());
    let wakes_before = wakes.load(Ordering::SeqCst);
    click(&mut el, &mut app, &d, 1);
    pump(&mut el, &mut app, 2_000, &|_| question.try_result().is_some());
    assert!(matches!(question.try_result(), Some(Ok(XDialogResult::ButtonPressed(1)))), "the click answered the dialog");
    assert!(wakes.load(Ordering::SeqCst) > wakes_before, "the answer woke the loop");
    assert!(matches!(question.wait(), Ok(XDialogResult::ButtonPressed(1))), "once answered, waiting doesn't block");
    let polled = std::pin::Pin::new(&mut question).poll(&mut cx);
    assert!(matches!(polled, std::task::Poll::Ready(Ok(XDialogResult::ButtonPressed(1)))));
    drop(question);

    // Dropping an unanswered proxy closes its dialog.
    let dropped = show_message(XDialogOptions { title: "dropped".into(), ..Default::default() });
    wait_titled(&mut el, &mut app, "dropped");
    drop(dropped);
    pump(&mut el, &mut app, 2_000, &|app| titled(app, "dropped").is_none());
    assert!(titled(&app, "dropped").is_none(), "dropping the proxy closes the dialog");

    // Escape closes a message.
    let worker = message("Escape");
    let d = wait_dialog(&mut el, &mut app);
    let escape = Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() };
    app.test_inject(d.id, escape);
    pump(&mut el, &mut app, 2_000, &|_| worker.is_finished());
    assert!(worker.is_finished(), "Escape answered the dialog");
    assert!(matches!(worker.join().unwrap(), Ok(XDialogResult::WindowClosed)));
    assert!(app.inner().resumes.is_empty(), "the host app never sees xdialog's deadline as its own");

    // Several worker threads at once: all their dialogs open, each closes by its timeout.
    let workers: Vec<_> = (0..4).map(|i| {
                                    let options = XDialogOptions { title: format!("worker {i}"), buttons: vec!["OK".into()], ..Default::default() };
                                    std::thread::spawn(move || show_message(options).wait_timeout(Duration::from_secs(2)))
                                })
                                .collect();
    pump(&mut el, &mut app, 10_000, &|app| app.test_dialogs().len() == 4);
    assert_eq!(app.test_dialogs().len(), 4, "every worker's dialog is open");
    pump(&mut el, &mut app, 10_000, &|_| workers.iter().all(|w| w.is_finished()));
    for w in workers {
        assert!(w.is_finished(), "the timeout closed the dialog");
        assert!(matches!(w.join().unwrap(), Ok(XDialogResult::TimeoutElapsed)));
    }

    // A progress callback runs on the event-loop thread: a message box there fails fast, another
    // progress dialog opens without waiting; `true` keeps its dialog open.
    let (tx, rx) = std::sync::mpsc::channel();
    let options = XDialogOptions { title: "callback".into(), buttons: vec!["Cancel".into()], ..Default::default() };
    let progress = show_progress_with_callback(options, move |i, proxy| {
                       let _ = proxy.set_text("Cancelling...");
                       let blocked = show_message_info_ok("t", "b", "c");
                       let nested = show_progress_ex(XDialogOptions { title: "nested".into(), buttons: vec!["Hide".into()], ..Default::default() });
                       let _ = tx.send((i, blocked, nested.is_ok()));
                       std::mem::forget(nested); // open until its button closes it
                       true
                   }).unwrap();
    let d = wait_titled(&mut el, &mut app, "callback");
    click(&mut el, &mut app, &d, 0);
    let nested = wait_titled(&mut el, &mut app, "nested");
    let (i, blocked, nested_ok) = rx.try_recv().expect("the callback ran");
    assert_eq!(i, 0);
    assert!(matches!(blocked, Err(XDialogError::BlockingCallOnUiThread)), "{blocked:?}");
    assert!(nested_ok);
    assert!(titled(&app, "callback").is_some(), "the callback kept its dialog open");
    // A button without a callback closes its dialog.
    click(&mut el, &mut app, &nested, 0);
    pump(&mut el, &mut app, 2_000, &|app| titled(app, "nested").is_none());
    assert!(titled(&app, "nested").is_none(), "the nested dialog closed");
    drop(progress);

    // A panicking callback closes only its dialog; the loop keeps serving (the next section).
    let options = XDialogOptions { title: "panic".into(), buttons: vec!["Boom".into()], ..Default::default() };
    let _panicking = show_progress_with_callback(options, |_, _| panic!("test panic in a progress callback")).unwrap();
    let d = wait_titled(&mut el, &mut app, "panic");
    click(&mut el, &mut app, &d, 0);
    pump(&mut el, &mut app, 2_000, &|app| app.test_dialogs().is_empty());
    assert!(app.test_dialogs().is_empty(), "the panicking callback's dialog closed");

    // The host exits: `exiting` closes open dialogs and ends the backend.
    let worker = message("Exit");
    wait_dialog(&mut el, &mut app);
    app.inner_mut().quit = true;
    let end = Instant::now() + Duration::from_secs(5);
    while !matches!(el.pump_app_events(Some(Duration::from_millis(5)), &mut app), PumpStatus::Exit(_)) {
        assert!(Instant::now() < end, "the loop exits");
    }
    assert!(app.inner().exited);
    assert!(matches!(worker.join().unwrap(), Ok(XDialogResult::WindowClosed)));
    assert!(app.test_dialogs().is_empty());
    assert!(matches!(show_progress("t", "a", "b", XDialogIcon::None), Err(XDialogError::NoBackendAvailable)));
}
