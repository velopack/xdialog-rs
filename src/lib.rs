//! # xdialog
//! [![Version](https://img.shields.io/crates/v/xdialog?style=flat-square)](https://crates.io/crates/xdialog)
//! [![License](https://img.shields.io/crates/l/xdialog?style=flat-square)](https://github.com/velopack/xdialog/blob/master/LICENSE)
//!
//! A cross-platform library for displaying native dialogs in Rust. On Windows and macOS, this
//! library uses native system dialogs (Win32 TaskDialog and AppKit). On Linux, the default backend
//! is a pure Rust software renderer (winit + tiny-skia) with no C/C++ build dependencies, making it
//! fully compatible with static musl builds. Optional GTK3 and FLTK backends are available via
//! feature flags. This allows for a simplified API and consistent behavior across platforms.
//!
//! This is not a replacement for a proper GUI framework. It is meant to be used for CLI / background
//! applications which occasionally need to show dialogs (such as alerts, or progress) to the user.
//!
//! It's main use-case is for the [Velopack](https://velopack.io) application installation and
//! update framework.
//!
//! ## Features
//! - Cross-platform: works on Windows, macOS, and Linux
//! - Native backends on Windows (Win32) and macOS (AppKit) with zero additional build dependencies
//! - Pure Rust software-rendered backend on Linux (no C/C++ dependencies, static musl compatible)
//! - Optional GTK3 and FLTK backends on Linux via feature flags
//! - Embedded font (Liberation Sans) - no system font dependencies on Linux
//! - Simple and consistent API across all platforms
//!
//! ## Installation
//!
//! Add the following to your `Cargo.toml`:
//! ```toml
//! [dependencies]
//! xdialog = "0" # replace with the latest version
//! ```
//!
//! Or, run the following command:
//! ```sh
//! cargo install xdialog
//! ```
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
//! - **Windows**: Native Win32 TaskDialog API
//! - **macOS**: Native AppKit dialogs
//! - **Linux** (default): Pure Rust software renderer using winit + tiny-skia + fontdue. No C/C++
//!   build dependencies, works with static musl linking, and embeds its own font.
//! - **Linux** (`gtk` feature): GTK3 backend. Requires `libgtk-3-dev` on Debian/Ubuntu.
//!   When enabled, GTK3 becomes the default backend with automatic fallback to the software
//!   renderer if GTK fails to initialize.
//! - **Linux** (`fltk` feature): FLTK backend. Requires cmake and X11/Wayland development
//!   libraries. Must be explicitly selected via
//!   [`XDialogBuilder::with_backend(XDialogBackend::Fltk)`](XDialogBuilder::with_backend).
//! - **Headless Linux**: When no X11 or Wayland display server is available, all dialog functions
//!   return [`XDialogError::NoBackendAvailable`]. The application continues running without panicking.
//!

#![warn(missing_docs)]

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
mod win32_direct;
#[cfg(all(windows, feature = "win32-direct"))]
pub use win32_direct::init_win32_direct;

#[cfg(all(target_os = "macos", feature = "maccf-direct"))]
mod maccf_direct;
#[cfg(all(target_os = "macos", feature = "maccf-direct"))]
pub use maccf_direct::init_maccf_direct;

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
pub enum XDialogError {
    #[error("xdialog backend not initialized")]
    NotInitialized,
    #[error("xdialog command returned no result: {0}")]
    NoResult(oneshot::RecvError),
    #[error("xdialog send to backend failed: {0}")]
    SendFailed(String),
    #[error("xdialog generic error: {0}")]
    SystemError(String),
    #[error("no display server available (X11 or Wayland required)")]
    NoBackendAvailable,
}
