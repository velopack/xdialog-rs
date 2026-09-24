//! Hosting xdialog inside an application that owns a **winit 0.29** event loop.
//!
//! xdialog is built WITHOUT its built-in winit (`default-features = false, features =
//! ["winit-host"]`), so `cargo tree` of this package shows winit 0.29 and no winit 0.30. Only
//! raw-window-handle 0.6 handles and xdialog's own event enum cross the boundary.
//!
//! ```text
//! cargo run --manifest-path examples/winit_host/Cargo.toml              # interactive demo
//! cargo run --manifest-path examples/winit_host/Cargo.toml -- --selftest   # scripted, injected input
//! ```
//!
//! Interactive: a worker thread shows a blocking yes/no dialog; in the main window press `M`
//! (blocking call on the host thread: returns `BlockingCallOnUiThread`) or `P` (progress dialog
//! from the host thread). Closing the main window with dialogs open shuts xdialog down cleanly.
//!
//! `--selftest` drives the same glue without user input: dialogs from a worker and from the host
//! thread, input injected through `xdialog::host::handle_event`, the blocking-call error path, and
//! exiting the loop with dialogs open. Windows are shown without activation.
//!
//! The glue itself (`HostWindows` impl, event translation) is in `lib.rs`.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use raw_window_handle::HasWindowHandle;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoopBuilder, EventLoopWindowTarget};
use winit::keyboard::Key as WKey;
use winit::window::{Window, WindowBuilder};
use xdialog::host::{self, HostEvent, Key, WindowKey};
use xdialog_winit_host_example::{translate, winit, Glue, XdWindows};
use xdialog::{XDialogError, XDialogIcon, XDialogOptions, XDialogTheme};

#[derive(Debug, Clone, Copy)]
enum AppEvent {
    /// xdialog asked us to iterate the loop (the pump happens in `AboutToWait`).
    XDialogWake,
}


fn main() {
    let selftest = std::env::args().any(|a| a == "--selftest");
    if selftest {
        // Never take focus from the user (honoured by xdialog in every build).
        std::env::set_var("XDIALOG_TEST_NO_ACTIVATE", "1");
    }

    let event_loop = EventLoopBuilder::<AppEvent>::with_user_event().build().unwrap();

    // 1. Install xdialog in host mode. The waker makes the loop iterate; `pump` runs in AboutToWait.
    let proxy = event_loop.create_proxy();
    xdialog::init_winit_host(XDialogTheme::SystemDefault, move || {
        let _ = proxy.send_event(AppEvent::XDialogWake);
    }).unwrap();

    // The application's own window (hidden and inactive in the self-test).
    let main_window = WindowBuilder::new().with_title("winit 0.29 host: press P (progress) or M (message)")
                                          .with_inner_size(LogicalSize::new(480.0, 200.0))
                                          .with_visible(!selftest)
                                          .with_active(!selftest)
                                          .build(&event_loop)
                                          .unwrap();

    let mut xd = XdWindows { owner: main_window.window_handle().ok().map(|h| h.as_raw()), ..Default::default() };
    let mut script = if selftest { Some(selftest::Script::new(&event_loop)) } else { None };

    if !selftest {
        // 2. Ordinary blocking xdialog calls from a worker thread just work.
        std::thread::spawn(|| {
            let yes = xdialog::show_message_yes_no("Host demo",
                                                   "Hosted by winit 0.29",
                                                   "This dialog is rendered by xdialog inside the host's event loop. Show a progress dialog?",
                                                   XDialogIcon::Information).unwrap();
            if yes {
                let p = xdialog::show_progress("Host demo", "Working", "Starting...", XDialogIcon::None).unwrap();
                for i in 1..=10 {
                    p.set_value(i as f32 / 10.0).unwrap();
                    p.set_text(format!("Step {i} of 10")).unwrap();
                    std::thread::sleep(Duration::from_millis(300));
                }
            }
        });
    }

    let mut host_progress = Vec::new();
    event_loop.run(|event, elwt| match event {
                  // Waking is enough: AboutToWait below pumps.
                  Event::UserEvent(AppEvent::XDialogWake) => {}

                  Event::WindowEvent { window_id, event } => {
                      // 3. Route xdialog's windows to xdialog.
                      if let Some(key) = xd.by_id.get(&window_id).copied() {
                          let mut glue = Glue { elwt, windows: &mut xd };
                          match event {
                              WindowEvent::RedrawRequested => host::redraw(&mut glue, key),
                              other => {
                                  if let Some(ev) = translate(&other) {
                                      host::handle_event(&mut glue, key, ev);
                                  }
                              }
                          }
                          return;
                      }
                      // The host's own window.
                      match event {
                          WindowEvent::CloseRequested if window_id == main_window.id() => elwt.exit(),
                          WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => match event.logical_key.as_ref() {
                              // Blocking call ON the host thread: returns an error instead of deadlocking.
                              WKey::Character("m") => {
                                  let r = xdialog::show_message_info_ok("Host demo", "Blocked", "never shown");
                                  eprintln!("show_message on the host thread -> {r:?}");
                              }
                              // Non-blocking on the host thread: the proxy is returned now and the
                              // window appears on the next pump.
                              WKey::Character("p") => {
                                  let p = xdialog::show_progress_ex(XDialogOptions { title: "Host demo".into(),
                                                                                     main_instruction: "From the host thread".into(),
                                                                                     message: "Indeterminate...".into(),
                                                                                     icon: XDialogIcon::None,
                                                                                     buttons: vec!["Close".into()] }).unwrap();
                                  p.set_indeterminate().unwrap();
                                  // Keep the proxy: dropping it closes the dialog.
                                  host_progress.push(p);
                              }
                              _ => {}
                          },
                          _ => {}
                      }
                  }

                  // 4. Pump every iteration and honour xdialog's deadline.
                  Event::AboutToWait => {
                      let next = host::pump(&mut Glue { elwt, windows: &mut xd });
                      let mut flow = match next {
                          Some(t) => ControlFlow::WaitUntil(t),
                          None => ControlFlow::Wait,
                      };
                      if let Some(s) = script.as_mut() {
                          s.step(elwt, &mut xd);
                          // The script polls its workers: wake up at least every 20 ms.
                          let poll = Instant::now() + Duration::from_millis(20);
                          flow = ControlFlow::WaitUntil(next.map_or(poll, |t| t.min(poll)));
                      }
                      elwt.set_control_flow(flow);
                  }

                  // 5. REQUIRED: release xdialog's surfaces and windows while the display is still
                  //    open. Open dialogs are closed; their callers receive WindowClosed.
                  Event::LoopExiting => host::shutdown(&mut Glue { elwt, windows: &mut xd }),
                  _ => {}
              })
              .unwrap();

    // After shutdown every dialog function fails fast.
    let after = xdialog::show_message_info_ok("Host demo", "After shutdown", "never shown");
    eprintln!("show_message after shutdown -> {after:?}");
    drop(host_progress);
    if let Some(s) = script {
        s.finish(&xd, after);
    }
}

/// The scripted run (`--selftest`).
mod selftest {
    use super::*;

    type MsgResult = Result<bool, XDialogError>;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Stage {
        /// A worker shows a blocking yes/no dialog.
        WorkerMessage,
        /// Its window exists: inject Enter.
        InjectEnter,
        /// Wait for the worker's result and the window to go away.
        WorkerResult,
        /// (feature `test-hooks`) A worker message answered by an injected mouse click.
        MouseClick,
        /// Blocking and non-blocking calls on the host thread.
        HostThreadCalls,
        /// Wait for the host-thread progress window; start more workers.
        HostProgress,
        /// A worker progress runs to completion; another worker's message stays open.
        Busy,
        /// Exit the loop with dialogs open.
        Exit,
    }

    pub(super) struct Script {
        stage: Stage,
        since: Instant,
        start: Instant,
        worker_msg: Option<mpsc::Receiver<MsgResult>>,
        worker_progress: Option<mpsc::Receiver<Result<(), XDialogError>>>,
        open_msg: Option<std::thread::JoinHandle<MsgResult>>,
        click_msg: Option<std::thread::JoinHandle<Result<xdialog::XDialogResult, XDialogError>>>,
        #[allow(dead_code)]
        clicked: bool,
        host_progress: Option<xdialog::ProgressDialogProxy>,
        /// Native handles of every dialog window seen (Windows: HWNDs).
        ours: Vec<isize>,
        checks: Vec<(String, bool)>,
        max_windows: usize,
    }

    impl Script {
        pub(super) fn new(el: &winit::event_loop::EventLoop<AppEvent>) -> Self {
            // Bottom-right corner of the primary monitor (PrintWindow-able, never activated), unless
            // the caller placed the windows. XDIALOG_TEST_POS is honoured in debug builds.
            if std::env::var_os("XDIALOG_TEST_POS").is_none() {
                if let Some(m) = el.primary_monitor() {
                    let (p, s) = (m.position(), m.size());
                    std::env::set_var("XDIALOG_TEST_POS", format!("{},{}", p.x + s.width as i32 - 640, p.y + s.height as i32 - 360));
                }
            }
            Script { stage: Stage::WorkerMessage,
                     since: Instant::now(),
                     start: Instant::now(),
                     worker_msg: None,
                     worker_progress: None,
                     open_msg: None,
                     click_msg: None,
                     clicked: false,
                     host_progress: None,
                     ours: Vec::new(),
                     checks: Vec::new(),
                     max_windows: 0 }
        }

        fn check(&mut self, what: impl Into<String>, ok: bool) {
            let what = what.into();
            eprintln!("[selftest] {} {what}", if ok { "ok  " } else { "FAIL" });
            self.checks.push((what, ok));
        }

        fn go(&mut self, stage: Stage) {
            eprintln!("[selftest] -> {stage:?} ({:.2}s)", self.start.elapsed().as_secs_f32());
            self.stage = stage;
            self.since = Instant::now();
        }

        fn inject(elwt: &EventLoopWindowTarget<AppEvent>, xd: &mut XdWindows, ev: HostEvent) {
            let keys: Vec<WindowKey> = xd.by_key.keys().copied().collect();
            for key in keys {
                host::handle_event(&mut Glue { elwt, windows: &mut *xd }, key, ev);
            }
        }

        pub(super) fn step(&mut self, elwt: &EventLoopWindowTarget<AppEvent>, xd: &mut XdWindows) {
            self.max_windows = self.max_windows.max(xd.by_key.len());
            for w in xd.by_key.values() {
                let h = native(w);
                if !self.ours.contains(&h) {
                    self.ours.push(h);
                }
            }
            let fg = foreground();
            if fg != 0 && self.ours.contains(&fg) {
                self.check("a dialog window became the foreground window", false);
            }
            if self.start.elapsed() > Duration::from_secs(30) {
                self.check(format!("timed out in stage {:?}", self.stage), false);
                elwt.exit();
                return;
            }
            let settled = self.since.elapsed() > Duration::from_millis(300);
            match self.stage {
                Stage::WorkerMessage => {
                    let (tx, rx) = mpsc::channel();
                    std::thread::spawn(move || {
                        let r = xdialog::show_message_yes_no("xdialog host selftest",
                                                             "From a worker thread",
                                                             "Enter is injected through xdialog::host::handle_event.",
                                                             XDialogIcon::Information);
                        let _ = tx.send(r);
                    });
                    self.worker_msg = Some(rx);
                    self.go(Stage::InjectEnter);
                }
                Stage::InjectEnter => {
                    if xd.by_key.len() == 1 && settled {
                        // Hover somewhere, leave, then Enter on the focused (default) button.
                        Self::inject(elwt, xd, HostEvent::CursorMoved { x: 20.0, y: 20.0 });
                        Self::inject(elwt, xd, HostEvent::CursorLeft);
                        Self::inject(elwt, xd, HostEvent::Key { key: Key::Enter, pressed: true, repeat: false });
                        Self::inject(elwt, xd, HostEvent::Key { key: Key::Enter, pressed: false, repeat: false });
                        self.go(Stage::WorkerResult);
                    }
                }
                Stage::WorkerResult => {
                    if let Some(r) = self.worker_msg.as_ref().and_then(|rx| rx.try_recv().ok()) {
                        self.check(format!("worker yes/no answered by injected Enter: {r:?}"), matches!(r, Ok(true)));
                        self.worker_msg = None;
                    }
                    if self.worker_msg.is_none() && xd.by_key.is_empty() {
                        self.check("dialog window destroyed after the answer", true);
                        if cfg!(feature = "test-hooks") {
                            self.click_msg = Some(std::thread::spawn(|| {
                                                      xdialog::show_message(XDialogOptions { title: "xdialog host selftest".into(),
                                                                                             main_instruction: "Mouse input".into(),
                                                                                             message: "The first button is clicked with injected pointer events.".into(),
                                                                                             icon: XDialogIcon::Warning,
                                                                                             buttons: vec!["First".into(), "Second".into(), "Third".into()] },
                                                                            None)
                                                  }));
                            self.go(Stage::MouseClick);
                        } else {
                            self.go(Stage::HostThreadCalls);
                        }
                    }
                }
                Stage::MouseClick => {
                    #[cfg(feature = "test-hooks")]
                    self.mouse_click(elwt, xd, settled);
                }
                Stage::HostThreadCalls => {
                    let r = xdialog::show_message_info_ok("xdialog host selftest", "Blocked", "never shown");
                    self.check(format!("show_message on the host thread -> {r:?}"), matches!(r, Err(XDialogError::BlockingCallOnUiThread)));
                    let t = Instant::now();
                    let p = xdialog::show_progress_ex(XDialogOptions { title: "xdialog host selftest".into(),
                                                                       main_instruction: "From the host thread".into(),
                                                                       message: "Indeterminate; closed by host::shutdown".into(),
                                                                       icon: XDialogIcon::Information,
                                                                       buttons: vec!["Close".into()] });
                    self.check(format!("show_progress on the host thread returns at once ({:?}): {}",
                                       t.elapsed(),
                                       if p.is_ok() { "Ok" } else { "Err" }),
                               p.is_ok() && t.elapsed() < Duration::from_millis(500));
                    if let Ok(p) = p {
                        let _ = p.set_indeterminate();
                        self.host_progress = Some(p);
                    }
                    self.check("no window before the next pump", xd.by_key.is_empty());
                    self.go(Stage::HostProgress);
                }
                Stage::HostProgress => {
                    if xd.by_key.len() == 1 && settled {
                        self.check("host-thread progress window created by the pump", true);
                        let (tx, rx) = mpsc::channel();
                        std::thread::spawn(move || {
                            let r = (|| {
                                let p = xdialog::show_progress("xdialog host selftest", "Worker progress", "Starting", XDialogIcon::None)?;
                                for i in 1..=10 {
                                    p.set_value(i as f32 / 10.0)?;
                                    p.set_text(format!("Step {i} of 10"))?;
                                    std::thread::sleep(Duration::from_millis(80));
                                }
                                Ok(())
                            })();
                            let _ = tx.send(r);
                        });
                        self.worker_progress = Some(rx);
                        self.open_msg = Some(std::thread::spawn(|| {
                                                 xdialog::show_message_ok_cancel("xdialog host selftest",
                                                                                 "Left open",
                                                                                 "The host exits its loop while this is open.",
                                                                                 XDialogIcon::Warning)
                                             }));
                        self.go(Stage::Busy);
                    }
                }
                Stage::Busy => {
                    if let Some(r) = self.worker_progress.as_ref().and_then(|rx| rx.try_recv().ok()) {
                        self.check(format!("worker progress ran: {r:?}"), r.is_ok());
                        self.worker_progress = None;
                    }
                    // The worker progress closed (proxy dropped); the host progress and the open
                    // message remain.
                    if self.worker_progress.is_none() && xd.by_key.len() == 2 && settled {
                        self.check(format!("three dialogs were open at once (max {})", self.max_windows), self.max_windows == 3);
                        self.check(format!("no dialog window was ever the foreground window ({} seen)", self.ours.len()),
                                   !self.checks.iter().any(|(_, ok)| !ok));
                        self.go(Stage::Exit);
                        elwt.exit();
                    }
                }
                Stage::Exit => {}
            }
        }

        /// Click the first button of the only dialog with pointer events at its live button rect
        /// (`xdialog::__test::live_dialogs`, physical px); verify it rendered first.
        #[cfg(feature = "test-hooks")]
        fn mouse_click(&mut self, elwt: &EventLoopWindowTarget<AppEvent>, xd: &mut XdWindows, settled: bool) {
            if !self.clicked {
                let live = xdialog::__test::live_dialogs();
                let (Some(d), true) = (live.first(), xd.by_key.len() == 1 && settled) else { return };
                let (w, win) = xd.by_key.values().next().map(|w| (w.inner_size(), native(w))).unwrap();
                self.check(format!("live dialog {}x{} px, window {}x{} px, {} frame(s), {} buttons",
                                   d.size_px.0, d.size_px.1, w.width, w.height, d.frames, d.button_rects_px.len()),
                           d.frames > 0 && d.size_px == (w.width, w.height) && d.button_rects_px.len() == 3);
                #[cfg(windows)]
                {
                    let (cw, ch, px) = capture::print_window(win);
                    let distinct = px.chunks(4).map(|p| (p[0], p[1], p[2])).collect::<std::collections::HashSet<_>>().len();
                    self.check(format!("PrintWindow capture {cw}x{ch} has {distinct} distinct colours"), distinct > 16);
                    if let Some(dir) = std::env::var_os("XDIALOG_HOST_CAPTURE") {
                        capture::save_bmp(&std::path::Path::new(&dir).join("host_message.bmp"), cw, ch, &px);
                    }
                }
                let _ = win;
                let r = d.button_rects_px[0];
                let (x, y) = ((r[0] + r[2] / 2.0) as f64, (r[1] + r[3] / 2.0) as f64);
                Self::inject(elwt, xd, HostEvent::CursorMoved { x, y });
                Self::inject(elwt, xd, HostEvent::MouseButton { button: xdialog::host::MouseButton::Primary, pressed: true });
                Self::inject(elwt, xd, HostEvent::MouseButton { button: xdialog::host::MouseButton::Primary, pressed: false });
                self.clicked = true;
                return;
            }
            if self.click_msg.as_ref().is_some_and(|h| h.is_finished()) && xd.by_key.is_empty() {
                let r = self.click_msg.take().unwrap().join().expect("worker panicked");
                self.check(format!("injected click on the first button -> {r:?}"), matches!(r, Ok(xdialog::XDialogResult::ButtonPressed(0))));
                self.go(Stage::HostThreadCalls);
            }
        }

        pub(super) fn finish(mut self, xd: &XdWindows, after: Result<(), XDialogError>) {
            self.check("every window destroyed by host::shutdown", xd.by_key.is_empty() && xd.by_id.is_empty());
            if let Some(h) = self.open_msg.take() {
                let r = h.join().expect("worker panicked");
                self.check(format!("open message answered with WindowClosed by shutdown: {r:?}"), matches!(r, Ok(false)));
            }
            if let Some(p) = self.host_progress.take() {
                let r = p.set_value(0.5);
                self.check(format!("host progress proxy after shutdown: {r:?}"), true);
            }
            self.check(format!("show_message after shutdown -> {after:?}"), matches!(after, Err(XDialogError::NoBackendAvailable)));
            let fg = foreground();
            self.check("foreground is not a dialog window at the end", fg == 0 || !self.ours.contains(&fg));
            let failed = self.checks.iter().filter(|(_, ok)| !ok).count();
            if failed == 0 {
                eprintln!("[selftest] PASS ({} checks, {:.2}s)", self.checks.len(), self.start.elapsed().as_secs_f32());
            } else {
                eprintln!("[selftest] FAILED: {failed} of {} checks", self.checks.len());
                std::process::exit(1);
            }
        }
    }

    /// `PrintWindow(PW_RENDERFULLCONTENT)` capture of a window's client area (no activation).
    #[cfg(all(windows, feature = "test-hooks"))]
    mod capture {
        use windows_sys::Win32::Foundation::{POINT, RECT};
        use windows_sys::Win32::Graphics::Gdi::{
            ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject,
            BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        };
        use windows_sys::Win32::Storage::Xps::PrintWindow;
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect};

        /// PW_RENDERFULLCONTENT (not exported by windows-sys 0.48).
        const PW_RENDERFULLCONTENT: u32 = 2;

        /// Client area as (width, height, BGRA rows top-down).
        pub(super) fn print_window(hwnd: isize) -> (usize, usize, Vec<u8>) {
            // SAFETY: GDI calls on a live window of this process; every object is released.
            unsafe {
                let (mut wr, mut cr): (RECT, RECT) = (std::mem::zeroed(), std::mem::zeroed());
                GetWindowRect(hwnd, &mut wr);
                GetClientRect(hwnd, &mut cr);
                let mut origin = POINT { x: 0, y: 0 };
                ClientToScreen(hwnd, &mut origin);
                let (ww, wh) = ((wr.right - wr.left) as usize, (wr.bottom - wr.top) as usize);
                let (cw, ch) = ((cr.right - cr.left) as usize, (cr.bottom - cr.top) as usize);
                let (ox, oy) = ((origin.x - wr.left).max(0) as usize, (origin.y - wr.top).max(0) as usize);
                let screen = GetDC(0);
                let mem = CreateCompatibleDC(screen);
                let bmp = CreateCompatibleBitmap(screen, ww as i32, wh as i32);
                let old = SelectObject(mem, bmp);
                PrintWindow(hwnd, mem, PW_RENDERFULLCONTENT);
                SelectObject(mem, old);
                let mut bi: BITMAPINFO = std::mem::zeroed();
                bi.bmiHeader = BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                                                  biWidth: ww as i32,
                                                  biHeight: -(wh as i32),
                                                  biPlanes: 1,
                                                  biBitCount: 32,
                                                  biCompression: BI_RGB as u32,
                                                  ..std::mem::zeroed() };
                let mut px = vec![0u8; ww * wh * 4];
                GetDIBits(mem, bmp, 0, wh as u32, px.as_mut_ptr().cast(), &mut bi, DIB_RGB_COLORS);
                DeleteObject(bmp);
                DeleteDC(mem);
                ReleaseDC(0, screen);
                let mut client = Vec::with_capacity(cw * ch * 4);
                for y in oy..(oy + ch).min(wh) {
                    let row = (y * ww + ox) * 4;
                    client.extend_from_slice(&px[row..row + cw.min(ww - ox) * 4]);
                }
                (cw, ch, client)
            }
        }

        /// Write BGRA rows (top-down) as a 32-bit BMP.
        pub(super) fn save_bmp(path: &std::path::Path, w: usize, h: usize, bgra: &[u8]) {
            let mut f = Vec::with_capacity(54 + bgra.len());
            f.extend_from_slice(b"BM");
            f.extend_from_slice(&((54 + bgra.len()) as u32).to_le_bytes());
            f.extend_from_slice(&0u32.to_le_bytes());
            f.extend_from_slice(&54u32.to_le_bytes());
            f.extend_from_slice(&40u32.to_le_bytes());
            f.extend_from_slice(&(w as i32).to_le_bytes());
            f.extend_from_slice(&(-(h as i32)).to_le_bytes());
            f.extend_from_slice(&1u16.to_le_bytes());
            f.extend_from_slice(&32u16.to_le_bytes());
            f.extend_from_slice(&[0u8; 24]);
            f.extend_from_slice(bgra);
            let _ = std::fs::write(path, f);
        }
    }

    /// Native window id (HWND on Windows, X11 window id, 0 otherwise).
    fn native(w: &Window) -> isize {
        match w.window_handle().map(|h| h.as_raw()) {
            Ok(raw_window_handle::RawWindowHandle::Win32(h)) => h.hwnd.get(),
            Ok(raw_window_handle::RawWindowHandle::Xlib(h)) => h.window as isize,
            Ok(raw_window_handle::RawWindowHandle::Xcb(h)) => h.window.get() as isize,
            _ => 0,
        }
    }

    /// The foreground window (Windows), to prove no dialog was activated. 0 elsewhere.
    fn foreground() -> isize {
        #[cfg(windows)]
        {
            // SAFETY: a plain query without arguments.
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }
        }
        #[cfg(not(windows))]
        {
            0
        }
    }
}
