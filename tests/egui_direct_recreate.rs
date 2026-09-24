//! linux-direct and winit's one-event-loop-per-process rule. Own binary
//! (`harness = false`) so the probe event loop is the only one in the process, built on `main`.
//!
//! 1. Lazy start: `init_linux_direct` runs BEFORE the probe `EventLoop` is built, and the build
//!    still succeeds (init creates no loop).
//! 2. The first dialog request then fails fast with `SystemError` carrying the `winit-host`
//!    guidance (the UI thread hits `RecreationAttempt`), and so does every later request.
//!
//! `cargo test --test egui_direct_recreate --features linux-direct`

use std::time::{Duration, Instant};

use xdialog::*;

fn opts() -> XDialogOptions {
    XDialogOptions { title: "never shown".into(),
                     main_instruction: "never shown".into(),
                     message: String::new(),
                     icon: XDialogIcon::Information,
                     buttons: vec!["OK".into()] }
}

fn main() {
    std::env::set_var("XDIALOG_TEST_NO_ACTIVATE", "1");
    std::env::set_var("XDIALOG_TEST_POS", "offscreen");

    init_linux_direct(XDialogTheme::SystemDefault);

    // Step 1: init_linux_direct took no winit slot.
    let probe = match winit::event_loop::EventLoop::new() {
        Ok(el) => el,
        Err(winit::error::EventLoopError::RecreationAttempt) => panic!("init_linux_direct created an event loop eagerly"),
        Err(e) => {
            // No display server (headless Linux CI): nothing else to check here.
            println!("skipped: could not build the probe event loop: {e}");
            return;
        }
    };
    println!("ok: probe event loop built after init_linux_direct");

    // Step 2: the lazily started UI thread can't build its loop.
    for attempt in 0..2 {
        let t0 = Instant::now();
        let r = show_message(opts(), None);
        match &r {
            Err(XDialogError::SystemError(s)) if s.contains("winit-host") => {}
            other => panic!("attempt {attempt}: expected SystemError with winit-host guidance, got {other:?}"),
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "attempt {attempt} took {:?}", t0.elapsed());
        println!("ok: attempt {attempt}: {}", r.unwrap_err());
    }
    let p = show_progress("never shown", "never shown", "", XDialogIcon::Information);
    assert!(matches!(p, Err(XDialogError::SystemError(_))), "{:?}", p.err());
    println!("ok: show_progress fails too");

    drop(probe);
    println!("egui_direct_recreate: all checks passed");
}
