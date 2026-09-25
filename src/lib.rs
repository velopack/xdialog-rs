//! # xdialog
//! [![Version](https://img.shields.io/crates/v/xdialog?style=flat-square)](https://crates.io/crates/xdialog)
//! [![License](https://img.shields.io/crates/l/xdialog?style=flat-square)](https://github.com/velopack/xdialog/blob/master/LICENSE)
//!
//! A cross-platform library for displaying native dialogs in Rust. On Windows and macOS, this
//! library uses native system dialogs (Win32 TaskDialog and AppKit). On Linux, the backend is a
//! pure Rust software renderer ([egui](https://github.com/emilk/egui) drawn by a CPU rasterizer and
//! presented through winit + softbuffer) with no C/C++ build dependencies, making it fully
//! compatible with static musl builds. This allows for a simplified API and consistent behavior
//! across platforms.
//!
//! This is not a replacement for a proper GUI framework. It is meant to be used for CLI / background
//! applications which occasionally need to show dialogs (such as alerts, or progress) to the user.
//!
//! It's main use-case is for the [Velopack](https://velopack.io) application installation and
//! update framework.
//!
//! ## Features
//! - Cross-platform: works on Windows, macOS, and Linux
//! - Native backends on Windows (Win32) and macOS (AppKit), with no C/C++ build dependencies
//! - Pure Rust software-rendered backend on Linux (no C/C++ dependencies, static musl compatible)
//! - Embedded font (Ubuntu) - no system font dependencies on Linux
//! - Optional: a WinUI 3 (Fluent) look on Windows, xdialog running on its own thread without a
//!   builder, or integration into an event loop your application already runs
//! - Simple and consistent API across all platforms
//!
//! ## Installation
//!
//! Add the following to your `Cargo.toml`:
//! ```toml
//! [dependencies]
//! xdialog = "4"
//! ```
//!
//! Rust 1.95 or newer is required on Linux (and on Windows with an egui feature), because of
//! egui 0.36.
//!
//! ## Usage
//! Since some platforms require UI to be run on the main thread, xdialog expects to own the
//! main thread, and will launch your core application logic in another thread.
//!
//! ```rust,no_run
//! use xdialog::*;
//!
//! fn main() {
//!   // Use run() for the simplest case:
//!   XDialogBuilder::new().run(your_main_logic);
//!
//!   // Or run_i32() to return a process exit code:
//!   // let code = XDialogBuilder::new().run_i32(your_main_logic_i32);
//!
//!   // Or run_result() for Result-based error handling:
//!   // let result = XDialogBuilder::new().run_result(your_main_logic_result);
//! }
//!
//! fn your_main_logic() {
//!
//!   // ... do something here
//!
//!   let should_update_now = show_message_yes_no(
//!     "My App Incorporated",
//!     "New version available",
//!     "Would you like to to the new version now?",
//!     XDialogIcon::Warning,
//!   ).unwrap();
//!
//!   if !should_update_now {
//!     return; // user declined the dialog
//!   }
//!
//!   // ... do something here
//!
//!   let progress = show_progress(
//!     "My App Incorporated",
//!     "Main instruction",
//!     "Body text",
//!     XDialogIcon::Information
//!   ).unwrap();
//!
//!   progress.set_value(0.5).unwrap();
//!   progress.set_text("Extracting...").unwrap();
//!   std::thread::sleep(std::time::Duration::from_secs(3));
//!
//!   progress.set_value(1.0).unwrap();
//!   progress.set_text("Updating...").unwrap();
//!   std::thread::sleep(std::time::Duration::from_secs(3));
//!
//!   progress.set_indeterminate().unwrap();
//!   progress.set_text("Wrapping Up...").unwrap();
//!   std::thread::sleep(std::time::Duration::from_secs(3));
//!
//!   progress.close().unwrap();
//! }
//! ```
//!
//! There are more examples in the `examples` directory.
//! ```sh
//! cargo run --example various_options
//! ```
//!
//! ## Backends
//!
//! | Platform | [`XDialogBuilder`] (default) | Optional |
//! |---|---|---|
//! | Windows | Win32 TaskDialog | `egui-fluent`: WinUI 3 (Fluent) look drawn with egui. `win32-direct`: `init_win32_direct()`, no builder |
//! | macOS | AppKit | `maccf-direct`: `init_maccf_direct()`, no builder |
//! | Linux | egui, the classic xdialog look (Ubuntu font, blue accent), own winit 0.30 loop | `linux-direct`: `init_linux_direct()`, no builder. `winit-host`: runs inside your event loop |
//!
//! - **Headless Linux**: When no X11 or Wayland display server is available, all dialog functions
//!   return [`XDialogError::NoBackendAvailable`]. The application continues running without panicking.
//! - The egui backends follow the system light/dark preference (Windows registry, the XDG desktop
//!   portal on Linux) unless [`XDialogBuilder::with_theme`] forces one; the Fluent look uses the
//!   Windows accent colour. They render in software only; there is no GPU dependency.
//!
//! ## Cargo features
//!
//! | Feature | Default | What it does |
//! |---|---|---|
//! | `builtin-winit` | yes | Lets xdialog own a winit 0.30 event loop: the Linux builder backend, `linux-direct` and `egui-fluent` need it. On Windows it also compiles winit 0.30 (Cargo features can't be per-target), but nothing uses or links it unless `egui-fluent` / `egui-ubuntu` is on; `default-features = false` avoids it. On macOS it does nothing. |
//! | `egui-fluent` | | Windows: [`XDialogBuilder`] uses the Fluent (WinUI 3 look) egui backend instead of Win32 TaskDialog. |
//! | `egui-ubuntu` | | Compiles the Ubuntu egui theme on Windows too, for development and testing (it doesn't change the Windows default). On Linux the theme is always compiled; this feature only adds `builtin-winit` (the builder's own loop). |
//! | `linux-direct` | | `init_linux_direct()`: no [`XDialogBuilder`] needed; xdialog starts its own UI thread with a winit 0.30 loop on the first dialog. Linux and Windows. |
//! | `winit-host` | | `xdialog::host` + `init_winit_host()`: xdialog renders into windows your application creates in its own event loop (winit 0.29, 0.30, 0.31, or anything with raw-window-handle 0.6). |
//! | `win32-direct` | | `init_win32_direct()` (Windows) |
//! | `maccf-direct` | | `init_maccf_direct()` (macOS) |
//!
//! **`egui-fluent` is a graph-wide switch.** Cargo unifies features, so if *any* crate in your
//! dependency graph enables it, every [`XDialogBuilder`] in the final binary uses the Fluent
//! backend on Windows. Libraries should leave this choice to the application.
//!
//! Only depend on the egui backends where you need them; resolver 2 ignores features of
//! target-specific dependencies on other targets, so Windows and macOS don't compile egui:
//!
//! ```toml
//! [dependencies]
//! xdialog = { version = "4" }
//!
//! [target.'cfg(target_os = "linux")'.dependencies]
//! xdialog = { version = "4", features = ["linux-direct"] }
//! ```
//!
//! ### Using your own event loop (no built-in winit)
//!
//! winit allows one event loop per process, so an application that already runs one can't also
//! run xdialog's. Turn the default features off, so xdialog compiles no winit at all, and enable
//! `winit-host`:
//!
//! ```toml
//! [dependencies]
//! xdialog = { version = "4", default-features = false, features = ["winit-host"] }
//! ```
//!
//! The `host` module documentation (feature `winit-host`) has the contract and the glue for
//! winit 0.29, 0.30 and 0.31; `examples/winit_host` in the repository is a complete winit 0.29
//! host. With `default-features = false` and no handler installed, [`XDialogBuilder`] on Linux
//! has no backend: dialog functions return [`XDialogError::NoBackendAvailable`] (your `main` still
//! runs). Host mode uses the Ubuntu look on every platform (use `win32-direct` for native Windows
//! dialogs); on macOS `host` is a stub whose `init_winit_host` returns `NoBackendAvailable`.
//!
//! ### Threads
//!
//! Dialog functions can be called from any thread. A *blocking* call (`show_message*`) made on
//! xdialog's own UI thread would deadlock, so it returns [`XDialogError::BlockingCallOnUiThread`]
//! instead. `show_progress*` there returns immediately and the window appears on the next loop
//! iteration. The UI thread is the event-loop thread of the egui backends (where their progress
//! button callbacks run), the host event-loop thread in `winit-host` mode, and on macOS the
//! thread running the AppKit loop (where AppKit button callbacks run). With Win32 TaskDialog
//! (the Windows default and `win32-direct`) every dialog runs on its own thread, so a callback may
//! call any dialog function, including `show_message*`.
//!
//! ## Limitations of the egui backends
//!
//! - **Emoji** are drawn as monochrome outlines on Windows (Segoe UI Emoji). On Linux only outline
//!   emoji fonts can be used; most distributions ship only the bitmap *Noto Color Emoji*, so emoji
//!   show as empty boxes there (the previous skia renderer drew colour emoji).
//! - **Accessibility:** the egui backends expose nothing to screen readers yet. The default Win32
//!   TaskDialog and AppKit backends are accessible; on Windows `egui-fluent` is an opt-in trade-off.
//! - **One winit loop per process:** [`XDialogBuilder`] on Linux, `egui-fluent` and `linux-direct`
//!   own a winit 0.30 event loop. An application with its own winit loop must use `winit-host`.
//! - **Complex scripts:** right-to-left text is reordered correctly, but shaping is limited to what
//!   egui's text engine does.

#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[macro_use]
extern crate log;

pub use message::*;
pub use model::*;
pub use progress::*;
use state::*;

mod backends;

mod channel;
use channel::send_request;
mod builder;
pub use builder::*;

#[cfg(all(windows, feature = "win32-direct"))]
pub use backends::win32_direct::init_win32_direct;

#[cfg(all(target_os = "macos", feature = "maccf-direct"))]
pub use backends::maccf_direct::init_maccf_direct;

#[cfg(xd_linux_direct)]
#[cfg_attr(docsrs, doc(cfg(feature = "linux-direct")))]
pub use backends::egui_core::direct::init_linux_direct;

#[cfg(any(xd_winit_host, xd_winit_host_stub))]
#[cfg_attr(docsrs, doc(cfg(feature = "winit-host")))]
pub use host::init_winit_host;

#[cfg(any(xd_winit_host, xd_winit_host_stub))]
#[cfg_attr(docsrs, doc(cfg(feature = "winit-host")))]
#[path = "backends/host_api.rs"]
pub mod host;

#[cfg(xd_test_hooks)]
#[doc(hidden)]
pub mod __test {
    //! Test-only hooks (offscreen rendering, live input injection). Not part of the public API.
    pub use crate::backends::egui_core::testhooks::api::*;
}

mod message;
mod model;
mod progress;
mod state;

/// Set the silent mode for the dialog. When silent mode is enabled, all dialog functions will
/// return `XDialogResult::SilentMode` without showing any dialogs.
pub fn set_silent_mode(silent: bool) {
    set_silent(silent);
}

#[allow(missing_docs)]
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum XDialogError {
    #[error("xdialog backend not initialized")]
    NotInitialized,
    #[error("xdialog command returned no result: {0}")]
    NoResult(oneshot::RecvError),
    #[error("xdialog send to backend failed: {0}")]
    SendFailed(String),
    #[error("xdialog generic error: {0}")]
    SystemError(String),
    /// No backend can show dialogs: no display server (X11 or Wayland) on Linux, no builder
    /// backend compiled in (Linux built without `builtin-winit`), the macOS `init_winit_host` stub,
    /// `xdialog::host::shutdown` already called, or the `linux-direct` event loop could not start.
    #[error("no xdialog backend available (no display server, backend not compiled in, or host mode shut down)")]
    NoBackendAvailable,
    /// A blocking dialog function (such as `show_message`) was called on the xdialog UI thread:
    /// from a progress button callback of the egui or AppKit backends, or on the host event-loop
    /// thread in `winit-host` mode. Waiting there would deadlock, so the call fails instead; call
    /// it from another thread. (Win32 TaskDialog callbacks run on a per-dialog thread and never
    /// get this error.)
    #[error("xdialog: blocking dialog call on the xdialog UI thread (dialog callback or host event-loop thread) would deadlock")]
    BlockingCallOnUiThread,
}
