use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

use crate::channel::{init_handler, send_request, Inbox, InboxHandler};
#[cfg(windows)]
use crate::channel::DialogRequestHandler;
use crate::model::{DialogMessageRequest, XDialogBackend, XDialogTheme};

#[cfg(windows)]
pub mod win32;

#[cfg(target_os = "macos")]
pub mod appkit;

#[cfg(all(target_os = "macos", feature = "maccf-direct"))]
pub mod maccf_direct;

/// The egui backend core (runtime, windows, rendering, input, animation, ...).
pub mod egui_core;

/// The Ubuntu look (xdialog 3.x's Linux design) on egui.
pub mod egui_ubuntu;

/// The Fluent look (WinUI 3 ContentDialog) on egui.
pub mod egui_fluent;

/// The concrete backend to run (never `Auto`) and whether a failing egui dialog falls back to
/// Win32 TaskDialog (`Auto` on Windows 10+). `None`: the backend can't run on this platform.
/// The hidden `XDIALOG_BACKEND` env var (tests, field diagnosis) replaces `requested`.
pub(crate) fn resolve(requested: XDialogBackend) -> Option<(XDialogBackend, bool)> {
    let requested = std::env::var("XDIALOG_BACKEND").ok().and_then(|v| parse_backend(&v)).unwrap_or(requested);
    let chosen = resolve_here(requested);
    if chosen.is_none() {
        warn!("xdialog: the {requested:?} backend can't run on this platform");
    }
    chosen
}

fn resolve_here(requested: XDialogBackend) -> Option<(XDialogBackend, bool)> {
    use XDialogBackend::*;
    match requested {
        #[cfg(windows)]
        Auto if egui_core::platform_win::windows_10_or_later() => Some((Fluent, true)),
        #[cfg(windows)]
        Auto | Win32 => Some((Win32, false)),
        #[cfg(target_os = "macos")]
        Auto | AppKit => Some((AppKit, false)),
        #[cfg(not(any(windows, target_os = "macos")))]
        Auto => Some((Ubuntu, false)),
        Fluent | Ubuntu => Some((requested, false)),
        _ => None,
    }
}

/// An `XDIALOG_BACKEND` value: a variant name (`auto`, `win32`, `fluent`, `ubuntu`, `appkit`; any
/// case). Empty or unknown: `None` (unknown values are logged).
fn parse_backend(value: &str) -> Option<XDialogBackend> {
    use XDialogBackend::*;
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let found = [Auto, Win32, Fluent, Ubuntu, AppKit].into_iter().find(|b| format!("{b:?}").eq_ignore_ascii_case(value));
    if found.is_none() {
        warn!("xdialog: XDIALOG_BACKEND={value} is unknown; ignored");
    }
    found
}

/// `XDialogBuilder::run_loop`: install the request handler of the backend `requested` resolves
/// to, run `main` on a new thread, serve the requests on this thread until `main` returns (egui:
/// the winit loop; AppKit: its loop; Win32 TaskDialog and "no backend" need no loop) and return
/// `main`'s result. A panic in `main` is resumed here once the backend stopped.
pub(crate) fn run_builder<T: Send + 'static>(requested: XDialogBackend, theme: XDialogTheme, main: fn() -> T) -> T {
    if crate::channel::handler_installed() {
        // `into_host_app` / `init_win32_direct` / ... ran first: their handler serves the requests.
        warn!("xdialog: a request handler is already installed; XDialogBuilder runs main without its own backend");
        return std::thread::spawn(main).join().unwrap_or_else(|p| resume_unwind(p));
    }
    // Per-monitor-v2 for this thread only, restored when the builder returns (user's main thread).
    #[cfg(windows)]
    let _dpi = egui_core::platform_win::ThreadDpiGuard::per_monitor_v2();

    let serve: Option<Box<dyn FnOnce()>> = match resolve(requested) {
        None => {
            init_handler(Box::new(InboxHandler(Inbox::closed())));
            None
        }
        #[cfg(windows)]
        Some((XDialogBackend::Win32, _)) => {
            init_handler(Box::new(win32::TaskDialogManager::new()));
            None
        }
        #[cfg(target_os = "macos")]
        Some((XDialogBackend::AppKit, _)) => {
            let (inbox, rx) = Inbox::new(Box::new(|| {}));
            init_handler(Box::new(InboxHandler(inbox)));
            Some(Box::new(move || appkit::run_loop(rx)))
        }
        Some((backend, fallback)) => match egui_core::event_loop::start(backend, fallback, theme) {
            Ok(run) => Some(run),
            Err(reason) => {
                warn!("xdialog: egui backend unavailable ({reason})");
                #[cfg(windows)]
                let handler: Box<dyn DialogRequestHandler> =
                    if fallback { Box::new(win32::TaskDialogManager::new()) } else { Box::new(InboxHandler(Inbox::closed())) };
                #[cfg(not(windows))]
                let handler = Box::new(InboxHandler(Inbox::closed()));
                init_handler(handler);
                None
            }
        },
    };
    run_main(main, serve)
}

/// Run `main` on a new thread (sending `ExitEventLoop` when it ends) while `serve` runs here.
fn run_main<T: Send + 'static>(main: fn() -> T, serve: Option<Box<dyn FnOnce()>>) -> T {
    let main = std::thread::spawn(move || {
        let result = catch_unwind(main);
        let _ = send_request(DialogMessageRequest::ExitEventLoop);
        result
    });
    if let Some(serve) = serve {
        if let Err(e) = catch_unwind(AssertUnwindSafe(serve)) {
            error!("xdialog: backend panicked: {e:?}");
        }
    }
    match main.join() {
        Ok(Ok(value)) => value,
        Ok(Err(payload)) | Err(payload) => resume_unwind(payload),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use XDialogBackend::*;

    #[test]
    fn parse_backend_names() {
        assert_eq!(parse_backend("fluent"), Some(Fluent));
        assert_eq!(parse_backend(" UBUNTU "), Some(Ubuntu));
        assert_eq!(parse_backend("Win32"), Some(Win32));
        assert_eq!(parse_backend("appkit"), Some(AppKit));
        assert_eq!(parse_backend("auto"), Some(Auto));
        assert_eq!(parse_backend(""), None);
        assert_eq!(parse_backend("gtk"), None);
    }

    #[test]
    fn resolve_per_platform() {
        assert_eq!(resolve_here(Fluent), Some((Fluent, false)));
        assert_eq!(resolve_here(Ubuntu), Some((Ubuntu, false)));
        #[cfg(windows)]
        {
            let win10 = egui_core::platform_win::windows_10_or_later();
            assert_eq!(resolve_here(Auto), Some(if win10 { (Fluent, true) } else { (Win32, false) }));
            assert_eq!(resolve_here(Win32), Some((Win32, false)));
            assert_eq!(resolve_here(AppKit), None);
        }
        #[cfg(target_os = "macos")]
        {
            assert_eq!(resolve_here(Auto), Some((AppKit, false)));
            assert_eq!(resolve_here(AppKit), Some((AppKit, false)));
            assert_eq!(resolve_here(Win32), None);
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            assert_eq!(resolve_here(Auto), Some((Ubuntu, false)));
            assert_eq!(resolve_here(Win32), None);
            assert_eq!(resolve_here(AppKit), None);
        }
    }
}
