//! Live-window harness for the egui backends (Windows).
//!
//! ```text
//! cargo run --release --example egui_live --features _test-hooks,fluent-egui,linux-egui[,linux-direct] -- \
//!     [--all | --mode builder|direct --theme linux|fluent [--dark]] [--out <dir>] [--filter <substr>]
//! ```
//!
//! Shows gallery variants (`examples/egui_gallery/{linux,fluent}.rs`) in REAL windows and proves
//! the window path matches the offscreen renderer: for each variant it shows the dialog from a
//! worker thread, freezes the dialog clock (`__test::freeze_clock`), replays the script through
//! `__test::inject` / the progress proxy at the scripted (frozen) times, captures the client area
//! with `PrintWindow(PW_RENDERFULLCONTENT)` and compares it with `OffscreenDialog` renders of the
//! same script (`<out>/live/<run>/<capture>.png`, `__offscreen.png`, `__side.png`, and
//! `<out>/live/<run>/live_report.csv`).
//!
//! Never steals focus: windows are created with `XDIALOG_TEST_NO_ACTIVATE=1`, placed in the
//! bottom-right corner of the primary monitor (`XDIALOG_TEST_POS`; PrintWindow cannot capture a
//! window that is off every monitor), input is injected through the test hooks only (the real
//! cursor/keyboard are never used), and `GetForegroundWindow()` is sampled before and after every
//! step: it must never be one of our windows. Every dialog is closed through
//! `inject(CloseRequested)`.
//!
//! winit allows one event loop per process and the request handler is a `OnceLock`, so each run
//! (mode × theme × light/dark) is its own process: `--all` (the default) re-spawns this binary
//! once per run and waits for each. `direct` needs the `linux-direct` feature.

#[path = "egui_gallery/model.rs"]
mod model;
#[path = "egui_gallery/compare.rs"]
mod compare;
#[path = "egui_gallery/linux.rs"]
mod linux;
#[path = "egui_gallery/fluent.rs"]
mod fluent;

pub use model::*;
pub use xdialog::__test::{HostEvent, Key, TestAppearance, TestKind, TestProgress};
pub use xdialog::{XDialogIcon, XDialogOptions};

/// Variants shown live (a cross-section: static, hover fade, press, keyboard focus, progress),
/// with at most `MAX_CAPTURES` captures each.
const LIVE_VARIANTS: &[&str] = &["warning_yesno", "hover_default", "pressed_secondary", "focus_tab", "progress_50", "progress_indeterminate", "yesno",
                                  "icon_warning", "btn_standard_pointerover", "transition_standard_normal_to_pointerover", "progress_050",
                                  "kbfocus_yesno", "cd_long_title", "edge_scroll"];
const MAX_CAPTURES: usize = 4;
/// Live dialogs run at `T0 + t` (frozen); offscreen replays the same shifted script.
const T0: f64 = 100.0;

#[cfg(not(windows))]
fn main() {
    eprintln!("egui_live: Windows only (PrintWindow capture)");
}

#[cfg(windows)]
fn main() {
    std::process::exit(win::main());
}

#[cfg(windows)]
mod win {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
    use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetForegroundWindow, GetSystemMetrics, GetWindowRect, PW_RENDERFULLCONTENT, SM_CXSCREEN, SM_CYSCREEN};
    use xdialog::__test::{self as t, LiveDialog, MouseButton};
    use xdialog::{XDialogBuilder, XDialogTheme};

    use super::compare::{self, Img};
    use super::*;

    const ENV_RUN: &str = "EGUI_LIVE_RUN"; // "<mode>:<theme>:<light|dark>"
    const ENV_OUT: &str = "EGUI_LIVE_OUT";
    const ENV_FILTER: &str = "EGUI_LIVE_FILTER";

    pub fn main() -> i32 {
        if let Ok(run) = std::env::var(ENV_RUN) {
            return child(&run);
        }
        let mut args = std::env::args().skip(1);
        let (mut mode, mut theme, mut dark, mut out, mut filter) = (None, None, false, PathBuf::from("target/egui_live"), None);
        while let Some(a) = args.next() {
            match a.as_str() {
                "--all" => {}
                "--mode" => mode = args.next(),
                "--theme" => theme = args.next(),
                "--dark" => dark = true,
                "--out" => out = PathBuf::from(args.next().unwrap_or_default()),
                "--filter" => filter = args.next(),
                other => {
                    eprintln!("egui_live: unknown argument {other}");
                    return 2;
                }
            }
        }
        let runs: Vec<String> = match (mode, theme) {
            (Some(m), Some(t)) => vec![format!("{m}:{t}:{}", theme_word(dark))],
            (m, t) => {
                let modes: Vec<String> = match m {
                    Some(m) => vec![m],
                    None if cfg!(feature = "linux-direct") => vec!["builder".into(), "direct".into()],
                    None => vec!["builder".into()],
                };
                let themes: Vec<String> = t.map_or(vec!["linux".into(), "fluent".into()], |t| vec![t]);
                let mut r = Vec::new();
                for m in &modes {
                    for th in &themes {
                        if m == "direct" && th != "linux" {
                            continue; // linux-direct has the Linux look only
                        }
                        for d in ["light", "dark"] {
                            r.push(format!("{m}:{th}:{d}"));
                        }
                    }
                }
                r
            }
        };
        let exe = std::env::current_exe().expect("current exe");
        let mut failed = 0;
        for run in runs {
            let (mode, theme, _) = split_run(&run);
            let mut cmd = std::process::Command::new(&exe);
            cmd.env(ENV_RUN, &run).env(ENV_OUT, &out).env("XDIALOG_TEST_NO_ACTIVATE", "1");
            if let Some(f) = &filter {
                cmd.env(ENV_FILTER, f);
            }
            if mode == "builder" {
                cmd.env("XDIALOG_BACKEND", theme);
            }
            // Deterministic accent: the refs' purple for fluent, none for the linux look.
            if theme == "fluent" {
                cmd.env("XDIALOG_TEST_ACCENT_PALETTE", "F0C0F4,DB9EE5,B763CB,A94DC1,8E3AA7,692782,400E59").env_remove("XDIALOG_TEST_ACCENT");
            } else {
                cmd.env("XDIALOG_TEST_ACCENT", "none");
            }
            // SAFETY: plain metrics queries.
            let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
            // The whole window must be on the monitor: softbuffer blits to the window DC, which is
            // clipped to the visible region, so an off-screen part would capture as blank. Leave
            // room for the tallest dialogs (Fluent caps the client height at 756 logical px).
            cmd.env("XDIALOG_TEST_POS", format!("{},{}", (sw - 640).max(0), (sh - 900).max(0)));
            println!("== {run}");
            match cmd.status() {
                Ok(s) if s.success() => {}
                Ok(s) => {
                    println!("== {run}: FAILED ({s})");
                    failed += 1;
                }
                Err(e) => {
                    println!("== {run}: could not start: {e}");
                    failed += 1;
                }
            }
        }
        if failed > 0 {
            1
        } else {
            0
        }
    }

    fn split_run(run: &str) -> (&str, &str, bool) {
        let mut p = run.split(':');
        (p.next().unwrap_or("builder"), p.next().unwrap_or("linux"), p.next() == Some("dark"))
    }

    fn child(run: &str) -> i32 {
        let (mode, theme, dark) = split_run(run);
        let xt = if dark { XDialogTheme::Dark } else { XDialogTheme::Light };
        match mode {
            "builder" => XDialogBuilder::new().with_theme(xt).run_i32(scenarios),
            #[cfg(feature = "linux-direct")]
            "direct" => {
                xdialog::init_linux_direct(xt);
                scenarios()
            }
            other => {
                eprintln!("egui_live: mode {other} not available in this build (theme {theme})");
                2
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Capture
    // ---------------------------------------------------------------------------------------------

    /// Client-area capture (RGBA): `PrintWindow(PW_RENDERFULLCONTENT)` of the whole window, cropped
    /// (`PW_CLIENTONLY` together with full content returns a blank image).
    fn capture(hwnd: isize) -> Result<Img, String> {
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);
        // SAFETY: plain GDI calls on a live window of this process; every object is released.
        unsafe {
            let (mut wr, mut cr) = (RECT::default(), RECT::default());
            GetWindowRect(hwnd, &mut wr).map_err(|e| e.to_string())?;
            GetClientRect(hwnd, &mut cr).map_err(|e| e.to_string())?;
            let mut origin = POINT::default();
            let _ = ClientToScreen(hwnd, &mut origin);
            let (ww, wh) = ((wr.right - wr.left) as usize, (wr.bottom - wr.top) as usize);
            let (cw, ch) = ((cr.right - cr.left) as usize, (cr.bottom - cr.top) as usize);
            let (ox, oy) = ((origin.x - wr.left).max(0) as usize, (origin.y - wr.top).max(0) as usize);
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, ww as i32, wh as i32);
            let old = SelectObject(mem, bmp.into());
            let ok = PrintWindow(hwnd, mem, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)).as_bool();
            let mut bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32,
                                                                    biWidth: ww as i32,
                                                                    biHeight: -(wh as i32),
                                                                    biPlanes: 1,
                                                                    biBitCount: 32,
                                                                    biCompression: BI_RGB.0,
                                                                    ..Default::default() },
                                      ..Default::default() };
            let mut px = vec![0u8; ww * wh * 4];
            SelectObject(mem, old);
            GetDIBits(mem, bmp, 0, wh as u32, Some(px.as_mut_ptr().cast()), &mut bi, DIB_RGB_COLORS);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            if !ok {
                return Err("PrintWindow failed".into());
            }
            let (cw, ch) = (cw.min(ww.saturating_sub(ox)), ch.min(wh.saturating_sub(oy)));
            let mut out = Vec::with_capacity(cw * ch * 4);
            for y in oy..oy + ch {
                for p in px[(y * ww + ox) * 4..(y * ww + ox + cw) * 4].chunks(4) {
                    out.extend_from_slice(&[p[2], p[1], p[0], 255]);
                }
            }
            Ok(Img { w: cw as u32, h: ch as u32, px: out })
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Driving live dialogs
    // ---------------------------------------------------------------------------------------------

    fn foreground() -> isize {
        // SAFETY: plain query.
        unsafe { GetForegroundWindow() }.0 as isize
    }

    fn info(id: usize) -> Option<LiveDialog> {
        t::live_dialogs().into_iter().find(|d| d.id == id)
    }

    fn wait<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> Result<T, String> {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return Ok(v);
            }
            if t0.elapsed() > Duration::from_secs(10) {
                return Err(format!("timed out waiting for {what}"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Run `f` (which makes the dialog repaint) and wait until a new frame was presented.
    fn and_frame(id: usize, f: impl FnOnce()) -> Result<LiveDialog, String> {
        let before = info(id).ok_or("dialog vanished")?.frames;
        f();
        let d = wait("a frame", || info(id).filter(|d| d.frames > before))?;
        // Let DWM pick up the presented frame before PrintWindow.
        std::thread::sleep(Duration::from_millis(40));
        Ok(d)
    }

    enum Handle {
        Message(std::thread::JoinHandle<Result<xdialog::XDialogResult, xdialog::XDialogError>>),
        Progress(xdialog::ProgressDialogProxy),
    }

    struct Guard {
        ours: Vec<isize>,
        violations: Vec<String>,
    }

    impl Guard {
        fn check(&mut self, step: &str) {
            let fg = foreground();
            if fg != 0 && self.ours.contains(&fg) {
                self.violations.push(format!("{step}: foreground became our window {fg:#x}"));
            }
        }
    }

    /// Apply one scripted action to a live dialog (physical px from the published button rects).
    fn apply(id: usize, h: &Handle, a: &Action) -> Result<(), String> {
        let d = info(id).ok_or("dialog vanished")?;
        let centre = |i: usize| -> Result<(f64, f64), String> {
            let r = d.button_rects_px.get(i).ok_or(format!("no button {i}"))?;
            Ok(((r[0] + r[2] / 2.0) as f64, (r[1] + r[3] / 2.0) as f64))
        };
        let key = |key: Key, pressed: bool| HostEvent::Key { key, pressed, repeat: false };
        let btn = |pressed: bool| HostEvent::MouseButton { button: MouseButton::Primary, pressed };
        let mut evs = Vec::new();
        match a {
            Action::Event(e) => evs.push(*e),
            Action::HoverButton(i) => {
                let (x, y) = centre(*i)?;
                evs.push(HostEvent::CursorMoved { x, y });
            }
            Action::PressButton(i) => {
                let (x, y) = centre(*i)?;
                evs.push(HostEvent::CursorMoved { x, y });
                evs.push(btn(true));
            }
            Action::Release => evs.push(btn(false)),
            Action::MoveTo(x, y) => {
                let (w, hh, p) = (d.size_px.0 as f64, d.size_px.1 as f64, d.ppp as f64);
                let fx = if *x < 0.0 { w + *x as f64 * p } else { *x as f64 * p };
                let fy = if *y < 0.0 { hh + *y as f64 * p } else { *y as f64 * p };
                evs.push(HostEvent::CursorMoved { x: fx, y: fy });
            }
            Action::Leave => evs.push(HostEvent::CursorLeft),
            Action::Key(k) => {
                evs.push(key(*k, true));
                evs.push(key(*k, false));
            }
            Action::KeyDown(k) => evs.push(key(*k, true)),
            Action::KeyUp(k) => evs.push(key(*k, false)),
            Action::WindowFocus(f) => evs.push(HostEvent::Focused(*f)),
            Action::Progress(_) | Action::SetText(_) => {
                let Handle::Progress(proxy) = h else { return Err("progress action on a message dialog".into()) };
                let mut r = Ok(());
                // The loop repaints after applying the request.
                and_frame(id, || {
                    r = match a {
                        Action::Progress(TestProgress::Value(v)) => proxy.set_value(*v),
                        Action::Progress(TestProgress::Indeterminate) => proxy.set_indeterminate(),
                        Action::SetText(s) => proxy.set_text(s),
                        _ => Ok(()),
                    }
                })?;
                return r.map_err(|e| e.to_string());
            }
            other => return Err(format!("action {other:?} is not available on live windows")),
        }
        for e in evs {
            and_frame(id, || t::inject(id, e))?;
        }
        Ok(())
    }

    /// Live replay of one variant; returns (suffix, capture) per capture.
    fn run_live(v: &Variant, g: &mut Guard) -> Result<Vec<(String, Img)>, String> {
        let known: Vec<usize> = t::live_dialogs().iter().map(|d| d.id).collect();
        let handle = match v.kind {
            TestKind::Message => {
                let o = v.options.clone();
                Handle::Message(std::thread::spawn(move || xdialog::show_message(o, None)))
            }
            TestKind::Progress => Handle::Progress(xdialog::show_progress_ex(v.options.clone()).map_err(|e| format!("show_progress_ex: {e}"))?),
        };
        let title = v.options.title.clone();
        let d = wait("the live dialog", || {
                    let found = t::live_dialogs().into_iter().find(|d| d.title == title && !known.contains(&d.id) && d.frames > 0);
                    // A message dialog whose `show_message` already returned never appears.
                    let gone = matches!(&handle, Handle::Message(j) if j.is_finished());
                    if found.is_none() && gone { Some(Err(())) } else { found.map(Ok) }
                })?;
        let d = match d {
            Ok(d) => d,
            Err(()) => {
                let Handle::Message(j) = handle else { unreachable!() };
                return Err(format!("show_message returned before a window appeared: {:?}", j.join()));
            }
        };
        let id = d.id;
        g.ours.push(d.raw_window);
        g.check("shown");
        let result = replay(v, id, &handle, g);
        // Close (also on failure) and join.
        t::inject(id, HostEvent::CloseRequested);
        match handle {
            Handle::Message(j) => {
                let _ = j.join();
            }
            Handle::Progress(p) => {
                let _ = p.close();
            }
        }
        let _ = wait("the window to close", || info(id).is_none().then_some(()));
        g.check("closed");
        result
    }

    fn replay(v: &Variant, id: usize, h: &Handle, g: &mut Guard) -> Result<Vec<(String, Img)>, String> {
        let mut times: Vec<f64> = v.script.iter().map(|s| s.0).chain(v.captures.iter().map(|c| c.0)).collect();
        times.sort_by(f64::total_cmp);
        times.dedup();
        let mut shots = Vec::new();
        for &tt in &times {
            and_frame(id, || t::freeze_clock(id, Some(T0 + tt)))?;
            for (_, a) in v.script.iter().filter(|s| s.0 == tt) {
                apply(id, h, a)?;
                g.check("inject");
            }
            for (_, sfx) in v.captures.iter().filter(|c| c.0 == tt) {
                let d = info(id).ok_or("dialog vanished")?;
                shots.push((sfx.clone(), capture(d.raw_window)?));
            }
        }
        Ok(shots)
    }

    /// The live cross-section of the theme's gallery variants, captures thinned out.
    fn live_variants(theme: &str, dark: bool) -> Vec<Variant> {
        let all = match theme {
            "fluent" => fluent::variants(),
            _ => linux::variants(),
        };
        let filter = std::env::var(ENV_FILTER).ok();
        let th = theme_word(dark);
        all.into_iter()
           .filter(|v| LIVE_VARIANTS.iter().any(|n| v.name == format!("{n}_{th}")))
           .filter(|v| filter.as_deref().is_none_or(|f| v.name.contains(f)))
           .filter(|v| v.ppp == 1.0 && !v.script.iter().any(|s| matches!(s.1, Action::Disable(..))))
           .map(|mut v| {
               // Keep the final capture and a few evenly spaced frames.
               let n = v.captures.len();
               if n > MAX_CAPTURES {
                   let step = n.div_ceil(MAX_CAPTURES - 1);
                   v.captures = v.captures.iter().enumerate().filter(|(i, c)| i % step == 0 || c.1.is_empty() || *i == n - 1).map(|(_, c)| c.clone()).collect();
               }
               v
           })
           .collect()
    }

    fn shifted(v: &Variant) -> Variant {
        let mut s = v.clone();
        for a in &mut s.script {
            a.0 += T0;
        }
        for c in &mut s.captures {
            c.0 += T0;
        }
        s
    }

    fn scenarios() -> i32 {
        let run = std::env::var(ENV_RUN).unwrap_or_default();
        let (_, theme, dark) = split_run(&run);
        let dir = Path::new(&std::env::var(ENV_OUT).unwrap_or_else(|_| "target/egui_live".into())).join("live").join(run.replace(':', "_"));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("egui_live: {}: {e}", dir.display());
            return 1;
        }
        let fg_before = foreground();
        let mut g = Guard { ours: Vec::new(), violations: Vec::new() };
        let mut csv = String::from("variant,frame,live_w,live_h,off_w,off_h,identical_pct,over10_pct,max_channel_diff\n");
        let (mut failures, mut captures, mut identical) = (0, 0, 0);
        for v in live_variants(theme, dark) {
            let live = match run_live(&v, &mut g) {
                Ok(s) => s,
                Err(e) => {
                    println!("  {}: FAIL {e}", v.name);
                    failures += 1;
                    continue;
                }
            };
            let off = match run_variant(theme, &shifted(&v)) {
                Ok(f) => f,
                Err(e) => {
                    println!("  {}: offscreen FAIL {e}", v.name);
                    failures += 1;
                    continue;
                }
            };
            for ((sfx, img), f) in live.iter().zip(&off) {
                let o = Img::from_frame(f);
                let stem = format!("{}{}", v.name, sfx);
                let _ = img.save(&dir.join(format!("{stem}.png")));
                let _ = o.save(&dir.join(format!("{stem}__offscreen.png")));
                let (d, heat) = compare::diff(img, &o);
                let _ = compare::side_by_side(img, Some(&o), Some(&heat)).save(&dir.join(format!("{stem}__side.png")));
                let (ow, oh) = (img.w.min(o.w), img.h.min(o.h));
                let (mut same, mut maxd) = (0u64, 0u8);
                for y in 0..oh {
                    for x in 0..ow {
                        let (a, b) = (img.rgb(x, y), o.rgb(x, y));
                        let m = (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap_or(0);
                        maxd = maxd.max(m);
                        same += (m == 0) as u64;
                    }
                }
                let area = (img.w.max(o.w) * img.h.max(o.h)).max(1) as f64;
                let same_pct = same as f64 * 100.0 / area;
                captures += 1;
                let exact = same_pct == 100.0;
                identical += exact as usize;
                println!("  {stem}: live {}x{} offscreen {}x{} identical {same_pct:.2}% (max diff {maxd}){}",
                         img.w,
                         img.h,
                         o.w,
                         o.h,
                         if exact { "" } else { "  <- differs" });
                csv.push_str(&format!("{},{},{},{},{},{},{same_pct:.3},{:.3},{maxd}\n", v.name, sfx, img.w, img.h, o.w, o.h, d.diff_pct));
            }
        }
        let _ = std::fs::write(dir.join("live_report.csv"), csv);
        let fg_after = foreground();
        println!("  {captures} captures, {identical} pixel-identical to offscreen; foreground {fg_before:#x} -> {fg_after:#x}; -> {}", dir.display());
        for v in &g.violations {
            println!("  FOCUS VIOLATION {v}");
        }
        if !g.violations.is_empty() || failures > 0 {
            1
        } else {
            0
        }
    }
}
