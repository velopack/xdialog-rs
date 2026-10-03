//! Host mode (`winit-host`) across several runs of one event loop: an `XDialogHost` wrapping a
//! different app value for each run, driven with `pump_app_events` (run 1, so the test can inject
//! input) and `run_app_on_demand` (the later runs). Input is injected with the `_test-hooks` API;
//! windows never activate and are placed off every monitor. `harness = false`: winit wants its
//! loop on the main thread. `tests/winit_host.rs` covers the single-run `XDialogApp`.
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
        eprintln!("winit_host_on_demand: timed out");
        std::process::exit(1);
    });
    run();
    println!("winit_host_on_demand: ok");
}

#[cfg(target_os = "macos")]
fn run() {
    // AppKit needs its own loop; `Auto` falls back to a drawn look.
    let appkit = XDialogBuilder::new().with_backend(XDialogBackend::AppKit).into_host(|| {});
    assert!(matches!(appkit, Err(XDialogError::NoBackendAvailable)));
    let host = XDialogBuilder::new().into_host(|| {}).expect("into_host");
    assert!(!host.is_shut_down());
}

#[cfg(not(target_os = "macos"))]
fn run() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use xdialog::__test::{Event, Point, PointerButton};
    use xdialog::host::winit::application::ApplicationHandler;
    use xdialog::host::winit::event::WindowEvent;
    use xdialog::host::winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use xdialog::host::winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
    use xdialog::host::winit::platform::run_on_demand::EventLoopExtRunOnDemand;
    use xdialog::host::winit::window::{Window, WindowId};
    use xdialog::host::{HostedApp, LiveDialog};

    /// The windows every run's app created (dropped in `exiting`, their late `Destroyed` can
    /// arrive in a later run): the only ids an app may see.
    static OWN: Mutex<Vec<WindowId>> = Mutex::new(Vec::new());

    /// The test's own handler, one value per run: no xdialog code.
    #[derive(Default)]
    struct Inner {
        /// A hidden, inactive window of the host's own.
        window: Option<Window>,
        /// User events forwarded (the waker's included).
        user_events: usize,
        /// Exit when set (by the test, or by a worker thread through `quit_flag`).
        quit: bool,
        quit_flag: Option<Arc<AtomicBool>>,
        exited: bool,
    }

    impl ApplicationHandler for Inner {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            if self.window.is_none() {
                let attrs = Window::default_attributes().with_visible(false).with_active(false).with_title("host");
                let window = el.create_window(attrs).unwrap();
                OWN.lock().unwrap().push(window.id());
                self.window = Some(window);
            }
        }

        fn user_event(&mut self, el: &ActiveEventLoop, (): ()) {
            self.user_events += 1;
            // The worker's "done" (winit on Windows waits out the control flow before it notices
            // an `exit()` made in `about_to_wait`, so exit from the event that asks for it).
            if self.quit_flag.as_ref().is_some_and(|f| f.load(Ordering::SeqCst)) {
                el.exit();
            }
        }

        fn window_event(&mut self, _: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
            assert!(OWN.lock().unwrap().contains(&id), "a dialog window's {event:?} reached the host app");
        }

        fn about_to_wait(&mut self, el: &ActiveEventLoop) {
            // A long wait of the host's own: a lost wake-up would stall, the pump's timeouts
            // bound the damage.
            el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_secs(3600)));
            if self.quit {
                el.exit();
            }
        }

        fn exiting(&mut self, _: &ActiveEventLoop) {
            self.exited = true;
            self.window = None;
        }
    }

    type App<'a> = HostedApp<'a, &'a mut Inner>;

    let mut el = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => return println!("winit_host_on_demand: skipped, no event loop: {e}"),
    };
    let proxy = el.create_proxy();
    let wakes = Arc::new(AtomicUsize::new(0));
    let w = wakes.clone();
    // Windows: Fluent without the TaskDialog fallback (TaskDialogs take focus).
    let backend = if cfg!(windows) { XDialogBackend::Fluent } else { XDialogBackend::Auto };
    let mut host = XDialogBuilder::new().with_backend(backend)
                                        .into_host(move || {
                                            w.fetch_add(1, Ordering::SeqCst);
                                            let _ = proxy.send_event(());
                                        })
                                        .expect("into_host");
    // One handler per process.
    assert!(matches!(XDialogBuilder::new().into_host(|| {}), Err(XDialogError::SystemError(_))));

    let message = |text: &str| {
        let options = XDialogOptions { title: text.into(), message: text.into(), buttons: vec!["OK".into()], ..Default::default() };
        std::thread::spawn(move || show_message(options).wait())
    };
    /// Pump for `ms`, or until `done`.
    fn pump(el: &mut EventLoop<()>, app: &mut App<'_>, ms: u64, done: &dyn Fn(&App<'_>) -> bool) {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end && !done(app) {
            el.pump_app_events(Some(Duration::from_millis(5)), app);
        }
    }
    fn titled(app: &App<'_>, title: &str) -> Option<LiveDialog> {
        app.test_dialogs().into_iter().find(|d| d.title == title)
    }
    fn wait_titled(el: &mut EventLoop<()>, app: &mut App<'_>, title: &str) -> LiveDialog {
        pump(el, app, 10_000, &|app| titled(app, title).is_some());
        titled(app, title).unwrap_or_else(|| panic!("dialog '{title}' opened"))
    }
    /// Click button `index` (move, press, release).
    fn click(el: &mut EventLoop<()>, app: &mut App<'_>, d: &LiveDialog, index: usize) {
        let [x, y, w, h] = d.button_rects[index];
        let pos = Point::new(x + w / 2.0, y + h / 2.0);
        let button = |pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed };
        app.test_inject(d.id, Event::PointerMoved(pos));
        app.test_inject(d.id, button(true));
        pump(el, app, 50, &|_| false);
        app.test_inject(d.id, button(false));
    }
    /// Pump until the loop exits (the app asked for it).
    fn pump_to_exit(el: &mut EventLoop<()>, app: &mut App<'_>) {
        let end = Instant::now() + Duration::from_secs(5);
        while !matches!(el.pump_app_events(Some(Duration::from_millis(5)), app), PumpStatus::Exit(_)) {
            assert!(Instant::now() < end, "the loop exits");
        }
    }

    // Run 1 (`pump_app_events`, so the test can inject input): a worker's dialog is answered by a
    // click; the wrapper is dropped mid-run and the host re-wrapped around another app, which
    // keeps serving the open dialog; the event-loop thread's progress dialog closes when its proxy
    // drops and the app's control flow comes back; the run ends with a dialog open, which closes.
    let mut first = Inner::default();
    let mut rewrapped = Inner::default();
    {
        let mut app = host.wrap(&mut first);
        let worker = message("run 1");
        let d = wait_titled(&mut el, &mut app, "run 1");
        click(&mut el, &mut app, &d, 0);
        pump(&mut el, &mut app, 2_000, &|_| worker.is_finished());
        assert!(matches!(worker.join().unwrap(), Ok(XDialogResult::ButtonPressed(0))), "the click answered the dialog");

        let worker = message("rewrap");
        let d = wait_titled(&mut el, &mut app, "rewrap");
        app.into_inner();
        let mut app = host.wrap(&mut rewrapped);
        assert!(titled(&app, "rewrap").is_some(), "the dialog outlives the wrapper");
        click(&mut el, &mut app, &d, 0);
        pump(&mut el, &mut app, 2_000, &|_| worker.is_finished());
        assert!(matches!(worker.join().unwrap(), Ok(XDialogResult::ButtonPressed(0))), "the new wrapper serves the dialog");

        let progress = show_progress("run 1 progress", "Run 1", "body", XDialogIcon::Information).unwrap();
        wait_titled(&mut el, &mut app, "run 1 progress");
        drop(progress);
        pump(&mut el, &mut app, 2_000, &|app| titled(app, "run 1 progress").is_none());
        assert!(titled(&app, "run 1 progress").is_none(), "dropping the proxy closes the dialog");
        pump(&mut el, &mut app, 100, &|_| false);
        let far = Instant::now() + Duration::from_secs(1800);
        assert!(matches!(app.test_control_flow(), Some(ControlFlow::WaitUntil(t)) if t > far), "idle: the host's flow is back");

        let worker = message("open at exit 1");
        wait_titled(&mut el, &mut app, "open at exit 1");
        app.inner_mut().quit = true;
        pump_to_exit(&mut el, &mut app);
        assert!(matches!(worker.join().unwrap(), Ok(XDialogResult::WindowClosed)), "the run's end closed the dialog");
        assert!(app.test_dialogs().is_empty());
    }
    assert!(rewrapped.exited && !first.exited, "`exiting` reached the app wrapped at the end");
    first.window = None; // winit: no window outlives a `run_app_on_demand` run
    assert!(!host.is_shut_down(), "the host outlives the run");

    // Between runs: a request (from the event-loop thread here: no blocking) queues and wakes the
    // loop for the next run.
    let wakes_before = wakes.load(Ordering::SeqCst);
    let between = show_message(XDialogOptions { title: "between runs".into(), buttons: vec!["OK".into()], ..Default::default() });
    assert!(between.try_result().is_none(), "queued until the next run");
    assert!(wakes.load(Ordering::SeqCst) > wakes_before, "a request between runs wakes the loop");

    // Run 2 (`run_app_on_demand`, another app value; the only re-run path from here on: winit
    // 0.30's `pump_app_events` doesn't clear the exit on X11/Wayland). A worker's `show_progress`
    // returns once its dialog is open (proof that this run served the queue), then it tells the
    // app to exit; the queued message dialog, open too, closes with the run. Run 1's closed
    // windows never reach the app (`Inner::window_event` panics on a foreign id).
    let done = Arc::new(AtomicBool::new(false));
    let mut second = Inner { quit_flag: Some(done.clone()), ..Default::default() };
    let proxy = el.create_proxy();
    let worker = std::thread::spawn(move || {
        let progress = show_progress("run 2", "Run 2", "body", XDialogIcon::None);
        let opened = progress.is_ok();
        drop(progress);
        done.store(true, Ordering::SeqCst);
        let _ = proxy.send_event(());
        opened
    });
    el.run_app_on_demand(&mut host.wrap(&mut second)).expect("run 2");
    assert!(worker.join().unwrap(), "the progress dialog opened in run 2");
    assert!(matches!(between.try_result(), Some(Ok(XDialogResult::WindowClosed))), "the queued dialog opened in run 2 and closed with it");
    assert!(second.exited);
    assert!(second.user_events > 0, "the wake-ups reached this run");
    assert!(host.test_dialogs().is_empty(), "the run's end closed the dialogs");

    // Shutdown between runs: the queue is rejected, later calls fail fast, and a run after that
    // only forwards (run 2's closed windows report `Destroyed` there, for the host to swallow).
    let queued = show_message(XDialogOptions { title: "queued at shutdown".into(), ..Default::default() });
    assert!(queued.try_result().is_none());
    host.shutdown();
    assert!(host.is_shut_down());
    assert!(matches!(queued.try_result(), Some(Err(XDialogError::NoBackendAvailable))), "the queue is rejected");
    assert!(matches!(show_progress("t", "a", "b", XDialogIcon::None), Err(XDialogError::NoBackendAvailable)));
    assert!(matches!(show_message_info_ok("t", "a", "b"), Err(XDialogError::NoBackendAvailable)), "not the UI thread any more");
    let done = Arc::new(AtomicBool::new(true));
    let mut third = Inner { quit_flag: Some(done), ..Default::default() };
    let _ = el.create_proxy().send_event(()); // exit from the first `user_event`
    el.run_app_on_demand(&mut host.wrap(&mut third)).expect("run 3");
    assert!(third.exited);
    drop(host);
    assert!(matches!(show_progress("t", "a", "b", XDialogIcon::None), Err(XDialogError::NoBackendAvailable)));
}
