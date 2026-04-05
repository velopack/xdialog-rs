//! # xdialog
//! [![Version](https://img.shields.io/crates/v/xdialog?style=flat-square)](https://crates.io/crates/xdialog)
//! [![License](https://img.shields.io/crates/l/xdialog?style=flat-square)](https://github.com/velopack/xdialog/blob/master/LICENSE)
//!
//! A cross-platform library for displaying native(-ish) dialogs in Rust. This library does not
//! use native system dialogs, but instead creates its own dialog windows which are designed to
//! look and feel like native dialogs. This allows for a simplified API and consistent behavior.
//!
//! This is not a replacement for a proper GUI framework. It is meant to be used for CLI / background
//! applications which occasionally need to show dialogs (such as alerts, or progress) to the user.
//!
//! It's main use-case is for the [Velopack](https://velopack.io) application installation and
//! update framework.
//!
//! ## Features
//! - Cross-platform: works on Windows, MacOS, and Linux
//! - Zero dependencies on Windows or MacOS, only requires X11 on Linux.
//! - Very small size (as little as 100kb added to your binary with optimal settings)
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
//! ```rust
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
//! ## Build Dependencies
//! This library uses [fltk-rs](https://github.com/fltk-rs/fltk-rs) for it's primary backend.
//! By default, [fltk-rs](https://github.com/fltk-rs/fltk-rs) provides pre-compiled binaries for
//! most platforms (win-x64, linux-x64, linux-arm64, mac-x64, mac-arm64).
//!
//! If you are compiling for a platform that does not have pre-compiled binaries, you will need
//! to disable the `fltk-bundled` feature and ensure that cmake is installed on your system.
//!
//! ```toml
//! [dependencies]
//! xdialog = { version = "0", default-features = false }
//!

#![warn(missing_docs)]

#[cfg(all(feature = "win32-direct", feature = "fltk"))]
compile_error!("The `win32-direct` and `fltk` features are mutually exclusive");

#[cfg(all(feature = "win32-direct", feature = "gtk3"))]
compile_error!("The `win32-direct` and `gtk3` features are mutually exclusive");

#[cfg(all(feature = "win32-direct", not(windows)))]
compile_error!("The `win32-direct` feature is only available on Windows");

#[cfg_attr(not(feature = "win32-direct"), macro_use)]
extern crate log;

pub use message::*;
pub use model::*;
pub use progress::*;
use state::*;

mod backends;

#[cfg(not(feature = "win32-direct"))]
mod channel;
#[cfg(not(feature = "win32-direct"))]
use channel::*;

#[cfg(not(feature = "win32-direct"))]
mod builder;
#[cfg(not(feature = "win32-direct"))]
pub use builder::*;

#[cfg(feature = "win32-direct")]
mod win32_direct;
#[cfg(feature = "win32-direct")]
use win32_direct::*;

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
    SendFailed(std::sync::mpsc::SendError<DialogMessageRequest>),
    #[error("xdialog generic error: {0}")]
    SystemError(String),
}
