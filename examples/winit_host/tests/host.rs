//! `xdialog::host` protocol test against a real winit 0.29 event loop.
//!
//! `init_winit_host` installs a process-wide handler, so everything runs in ONE `main`
//! (`harness = false`), driven step by step with `pump_events`:
//! wake coalescing and wake-after-drain, the host-thread blocking guard, input injected through
//! `host::handle_event` (and the deadline wake it causes), a re-entrant `handle_event` from inside
//! `HostWindows::set_visible`, a panicking progress callback, a wrong-thread `pump`,
//! `destroy_window` ordering, and `shutdown` with dialogs open.
//!
//! Windows are shown without activation (`XDIALOG_TEST_NO_ACTIVATE=1`) in the bottom-right
//! corner of the primary monitor. Run: `cargo test --manifest-path examples/winit_host/Cargo.toml`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopWindowTarget};
use winit::platform::pump_events::EventLoopExtPumpEvents;
use xdialog::host::{self, HostEvent, HostWindow, HostWindows, Key, WindowKey, WindowRequest};
use xdialog::{XDialogError, XDialogIcon, XDialogOptions, XDialogResult, XDialogTheme};
use xdialog_winit_host_example::{translate, winit, Glue, XdWindows};

#[derive(Debug, Clone, Copy)]
struct Wake;

/// Records every `HostWindows` call and optionally re-enters `xdialog::host` from `set_visible`.
struct Recorder<'a> {
    glue: Glue<'a, Wake>,
    log: &'a mut Vec<String>,
    reenter: bool,
}

// SAFETY: delegates to `Glue` (see its SAFETY comment); `shutdown` is called before the loop and
// the window table are dropped.
unsafe impl HostWindows for Recorder<'_> {
    fn create_window(&mut self, request: &WindowRequest) -> Result<HostWindow, String> {
        self.log.push(format!("create {:?}", request.key));
        self.glue.create_window(request)
    }
    fn set_visible(&mut self, key: WindowKey, visible: bool) {
        self.log.push(format!("visible {key:?} {visible}"));
        self.glue.set_visible(key, visible);
        if visible && self.reenter {
            // A toolkit that delivers events synchronously: re-enter while xdialog is busy.
            self.log.push(format!("reenter {key:?}"));
            host::handle_event(self, key, HostEvent::CursorMoved { x: 1.0, y: 1.0 });
            host::redraw(self, key);
            let _ = host::pump(self);
        }
    }
    fn set_inner_size(&mut self, key: WindowKey, width: f64, height: f64) {
        self.log.push(format!("size {key:?} {width}x{height}"));
        self.glue.set_inner_size(key, width, height);
    }
    fn request_redraw(&mut self, key: WindowKey) {
        self.glue.request_redraw(key);
    }
    fn destroy_window(&mut self, key: WindowKey) {
        self.log.push(format!("destroy {key:?}"));
        self.glue.destroy_window(key);
    }
}

struct Host {
    el: EventLoop<Wake>,
    xd: XdWindows,
    log: Vec<String>,
    reenter: bool,
    wakes: Arc<AtomicUsize>,
}

impl Host {
    fn with<R>(&mut self, f: impl FnOnce(&mut Recorder<'_>) -> R) -> R {
        let target: &EventLoopWindowTarget<Wake> = &self.el;
        let mut rec = Recorder { glue: Glue { elwt: target, windows: &mut self.xd }, log: &mut self.log, reenter: self.reenter };
        f(&mut rec)
    }

    /// One loop iteration (up to 10 ms of waiting), routed exactly like `main.rs`.
    fn iterate(&mut self) {
        let Host { el, xd, log, reenter, .. } = self;
        let reenter = *reenter;
        let _ = el.pump_events(Some(Duration::from_millis(10)), |event, elwt| {
                      let mut rec = Recorder { glue: Glue { elwt, windows: &mut *xd }, log: &mut *log, reenter };
                      match event {
                          Event::WindowEvent { window_id, event } => {
                              let Some(key) = rec.glue.windows.by_id.get(&window_id).copied() else { return };
                              match event {
                                  WindowEvent::RedrawRequested => host::redraw(&mut rec, key),
                                  other => {
                                      if let Some(ev) = translate(&other) {
                                          host::handle_event(&mut rec, key, ev);
                                      }
                                  }
                              }
                          }
                          Event::AboutToWait => {
                              let next = host::pump(&mut rec);
                              elwt.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
                          }
                          _ => {}
                      }
                  });
    }

    /// Iterate until `done` (panics after 10 s).
    fn until(&mut self, what: &str, mut done: impl FnMut(&mut Host) -> bool) {
        let t0 = Instant::now();
        while !done(self) {
            assert!(t0.elapsed() < Duration::from_secs(10), "timed out waiting for {what}");
            self.iterate();
        }
    }

    /// Iterate for `d`.
    fn run_for(&mut self, d: Duration) {
        let t0 = Instant::now();
        while t0.elapsed() < d {
            self.iterate();
        }
    }

    fn keys(&self) -> Vec<WindowKey> {
        let mut k: Vec<WindowKey> = self.xd.by_key.keys().copied().collect();
        k.sort_by_key(|k| format!("{k:?}"));
        k
    }

    fn inject_all(&mut self, ev: HostEvent) {
        for key in self.keys() {
            self.with(|r| host::handle_event(r, key, ev));
        }
    }

    fn wakes(&self) -> usize {
        self.wakes.load(Ordering::SeqCst)
    }
}

fn opts(heading: &str, buttons: &[&str]) -> XDialogOptions {
    XDialogOptions { title: "xdialog host test".into(),
                     main_instruction: heading.into(),
                     message: "Driven by examples/winit_host/tests/host.rs".into(),
                     icon: XDialogIcon::Information,
                     buttons: buttons.iter().map(|s| s.to_string()).collect() }
}

fn step(name: &str) {
    eprintln!("[host test] {name}");
}

fn enter() -> [HostEvent; 2] {
    [HostEvent::Key { key: Key::Enter, pressed: true, repeat: false }, HostEvent::Key { key: Key::Enter, pressed: false, repeat: false }]
}

fn main() {
    std::env::set_var("XDIALOG_TEST_NO_ACTIVATE", "1");

    let mut builder = EventLoopBuilder::<Wake>::with_user_event();
    #[cfg(windows)]
    winit::platform::windows::EventLoopBuilderExtWindows::with_any_thread(&mut builder, true);
    #[cfg(target_os = "linux")]
    winit::platform::x11::EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
    let el = match builder.build() {
        Ok(el) => el,
        Err(e) => {
            // No display server (headless CI): nothing to test here.
            eprintln!("[host test] SKIPPED: no event loop ({e})");
            return;
        }
    };
    if std::env::var_os("XDIALOG_TEST_POS").is_none() {
        if let Some(m) = el.primary_monitor() {
            let (p, s) = (m.position(), m.size());
            std::env::set_var("XDIALOG_TEST_POS", format!("{},{}", p.x + s.width as i32 - 640, p.y + s.height as i32 - 360));
        }
    }

    // Uninitialized: the entry points do nothing.
    let mut h = Host { el, xd: XdWindows::default(), log: Vec::new(), reenter: false, wakes: Arc::new(AtomicUsize::new(0)) };
    assert_eq!(h.with(|r| host::pump(r)), None);

    step("init");
    let proxy = h.el.create_proxy();
    let counter = h.wakes.clone();
    xdialog::init_winit_host(XDialogTheme::Light, move || {
        counter.fetch_add(1, Ordering::SeqCst);
        let _ = proxy.send_event(Wake);
    }).unwrap();
    assert!(matches!(xdialog::init_winit_host(XDialogTheme::Light, || {}), Err(XDialogError::SystemError(_))), "second init is rejected");

    // ---- wake coalescing: several requests before a pump -> one waker call -----------------------
    step("wake coalescing");
    let (tx, rx) = mpsc::channel();
    for i in 0..3 {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let r = xdialog::show_message(opts(&format!("Worker {i}"), &["Cancel", "OK"]), None);
            let _ = tx.send(r);
        });
    }
    let t0 = Instant::now();
    while h.wakes() == 0 || t0.elapsed() < Duration::from_millis(200) {
        std::thread::sleep(Duration::from_millis(10));
        assert!(t0.elapsed() < Duration::from_secs(5), "no wake");
    }
    assert_eq!(h.wakes(), 1, "three requests before a pump coalesce into one wake");
    h.until("three windows", |h| h.xd.by_key.len() == 3);

    // ---- input through handle_event answers the dialogs; handle_event wakes for its frame --------
    step("keyboard injection + deadline wake");
    h.run_for(Duration::from_millis(200));
    let before = h.wakes();
    for key in h.keys() {
        h.with(|r| host::handle_event(r, key, HostEvent::CursorMoved { x: 5.0, y: 5.0 }));
    }
    // The hover repaint is requested directly (request_redraw); the tween it starts then needs
    // animation frames, which the host is told about through the waker.
    h.until("a wake for the hover animation", |h| h.wakes() > before);
    for ev in enter() {
        h.inject_all(ev);
    }
    let mut results = Vec::new();
    h.until("three answers", |_| {
         while let Ok(r) = rx.try_recv() {
             results.push(r);
         }
         results.len() == 3
     });
    for r in &results {
        assert!(matches!(r, Ok(XDialogResult::ButtonPressed(1))), "Enter activates the default button: {r:?}");
    }
    h.until("windows destroyed", |h| h.xd.by_key.is_empty());

    // ---- wake after drain -----------------------------------------------------------------------
    step("wake after drain");
    h.iterate();
    let before = h.wakes();
    let worker = std::thread::spawn(|| xdialog::show_message(opts("After drain", &["OK"]), None));
    h.until("the wake", |h| h.wakes() > before);
    h.until("the window", |h| h.xd.by_key.len() == 1);

    // ---- re-entrancy: handle_event/redraw/pump from inside set_visible ---------------------------
    step("re-entrant calls from set_visible");
    h.reenter = true;
    let reentrant = std::thread::spawn(|| xdialog::show_message(opts("Re-entrant", &["OK"]), None));
    h.until("the re-entrant window", |h| h.xd.by_key.len() == 2);
    h.reenter = false;
    assert!(h.log.iter().any(|l| l.starts_with("reenter")), "set_visible re-entered: {:?}", h.log);
    h.run_for(Duration::from_millis(100));
    for ev in enter() {
        h.inject_all(ev);
    }
    h.until("both closed", |h| h.xd.by_key.is_empty());
    assert!(matches!(worker.join().unwrap(), Ok(XDialogResult::ButtonPressed(0))));
    assert!(matches!(reentrant.join().unwrap(), Ok(XDialogResult::ButtonPressed(0))));

    // ---- blocking guard on the host thread ------------------------------------------------------
    step("host-thread calls");
    let r = xdialog::show_message_info_ok("xdialog host test", "Blocked", "never shown");
    assert!(matches!(r, Err(XDialogError::BlockingCallOnUiThread)), "{r:?}");
    let t = Instant::now();
    let p = xdialog::show_progress("xdialog host test", "From the host thread", "Indeterminate", XDialogIcon::None).unwrap();
    assert!(t.elapsed() < Duration::from_millis(500), "show_progress on the host thread does not wait");
    p.set_indeterminate().unwrap();
    assert!(h.xd.by_key.is_empty(), "the window appears on the next pump");
    h.until("the host progress window", |h| h.xd.by_key.len() == 1);
    // An indeterminate bar keeps asking for frames.
    let deadline = h.with(|r| host::pump(r));
    assert!(deadline.is_some(), "an animating dialog returns a deadline");
    drop(p);
    h.until("progress closed by dropping its proxy", |h| h.xd.by_key.is_empty());

    // ---- a panicking progress callback never unwinds into the loop -------------------------------
    step("panicking progress callback");
    let (ptx, prx) = mpsc::channel();
    std::thread::spawn(move || {
        let p = xdialog::show_progress_with_callback(opts("Callback", &["Boom"]), |_, _| panic!("callback panic (expected)")).unwrap();
        // Keep the proxy until the dialog is gone.
        let _ = prx.recv();
        drop(p);
    });
    h.until("the callback window", |h| h.xd.by_key.len() == 1);
    h.run_for(Duration::from_millis(100));
    for ev in enter() {
        h.inject_all(ev);
    }
    h.until("the callback dialog closed", |h| h.xd.by_key.is_empty());
    let _ = ptx.send(());

    // ---- wrong-thread pump: queued creations fail (release) / debug_assert (debug) ---------------
    step("wrong-thread pump");
    h.iterate();
    let before = h.wakes();
    let queued = std::thread::spawn(|| xdialog::show_message(opts("Queued", &["OK"]), None));
    let t0 = Instant::now();
    while h.wakes() == before {
        assert!(t0.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(5));
    }
    let wrong = std::thread::spawn(|| {
                    struct Nothing;
                    // SAFETY: never creates windows.
                    unsafe impl HostWindows for Nothing {
                        fn create_window(&mut self, _: &WindowRequest) -> Result<HostWindow, String> {
                            Err("none".into())
                        }
                        fn set_visible(&mut self, _: WindowKey, _: bool) {}
                        fn set_inner_size(&mut self, _: WindowKey, _: f64, _: f64) {}
                        fn request_redraw(&mut self, _: WindowKey) {}
                        fn destroy_window(&mut self, _: WindowKey) {}
                    }
                    std::panic::catch_unwind(|| host::pump(&mut Nothing)).is_err()
                }).join()
                  .unwrap();
    assert_eq!(wrong, cfg!(debug_assertions), "wrong-thread pump panics exactly in debug builds");
    let r = queued.join().unwrap();
    assert!(matches!(r, Err(XDialogError::SystemError(_))), "queued creation failed by the wrong-thread pump: {r:?}");

    // ---- shutdown with dialogs open ---------------------------------------------------------------
    step("shutdown with dialogs open");
    let open_msg = std::thread::spawn(|| xdialog::show_message_ok_cancel("xdialog host test", "Open", "closed by shutdown", XDialogIcon::Warning));
    let (ktx, krx) = mpsc::channel::<()>();
    let open_progress = std::thread::spawn(move || {
        let p = xdialog::show_progress("xdialog host test", "Open progress", "closed by shutdown", XDialogIcon::None).unwrap();
        let _ = krx.recv();
        // After shutdown the proxy's updates are ignored.
        p.set_value(0.5)
    });
    h.until("two open dialogs", |h| h.xd.by_key.len() == 2);
    h.run_for(Duration::from_millis(100));
    let keys = h.keys();
    h.log.clear();
    h.with(|r| host::shutdown(r));
    assert!(h.xd.by_key.is_empty() && h.xd.by_id.is_empty(), "every window destroyed by shutdown");
    for key in &keys {
        let k = format!("{key:?}");
        let hide = h.log.iter().position(|l| *l == format!("visible {k} false"));
        let destroy = h.log.iter().position(|l| *l == format!("destroy {k}"));
        assert!(matches!((hide, destroy), (Some(a), Some(b)) if a < b), "hidden, then destroyed: {:?}", h.log);
        assert_eq!(h.log.iter().rfind(|l| l.contains(&k)), Some(&format!("destroy {k}")), "nothing after destroy_window");
    }
    assert!(matches!(open_msg.join().unwrap(), Ok(false)), "the open message got WindowClosed");
    let _ = ktx.send(());
    let r = open_progress.join().unwrap();
    eprintln!("[host test] progress proxy after shutdown: {r:?}");
    let r = std::thread::spawn(|| xdialog::show_message_info_ok("xdialog host test", "After", "never shown")).join().unwrap();
    assert!(matches!(r, Err(XDialogError::NoBackendAvailable)), "after shutdown: {r:?}");
    assert!(matches!(xdialog::show_message_info_ok("t", "h", "b"), Err(XDialogError::NoBackendAvailable)),
            "the host thread is no longer a UI thread after shutdown");
    h.with(|r| host::shutdown(r)); // idempotent
    assert_eq!(h.with(|r| host::pump(r)), None);
    h.run_for(Duration::from_millis(50));

    eprintln!("[host test] PASS");
}
