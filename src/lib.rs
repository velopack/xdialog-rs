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
//!   XDialogBuilder::new().run(your_main_logic);
//! }
//!
//! fn your_main_logic() -> i32 {
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
//!     return -1; // user declined the dialog
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
//!   0 // return exit code
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

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;

use std::sync::mpsc::channel;

pub use message::*;
pub use model::*;
pub use progress::*;
use state::*;

use crate::backends::XDialogBackendImpl;

pub mod errors;
mod model;
mod backends;
mod state;
mod images;
mod progress;
mod message;

#[derive(Debug)]
/// Builder pattern to configure/initialise the XDialog library. Must be configured and `run` in 
/// the main thread before any other XDialog functions are called.
pub struct XDialogBuilder
{
    backend: XDialogBackend,
    theme: XDialogTheme,
}

impl Default for XDialogBuilder
{
    fn default() -> XDialogBuilder
    {
        XDialogBuilder {
            backend: XDialogBackend::Automatic,
            theme: XDialogTheme::SystemDefault,
        }
    }
}

impl XDialogBuilder
{
    /// Create a new XDialogBuilder
    pub fn new() -> XDialogBuilder
    {
        XDialogBuilder::default()
    }

    /// Set the backend to use for the dialog. By default, the backend is chosen automatically.
    pub fn with_backend(mut self, backend: XDialogBackend) -> XDialogBuilder
    {
        self.backend = backend;
        self
    }

    /// Set the theme to use for the dialog. By default, the theme is chosen automatically.
    pub fn with_theme(mut self, theme: XDialogTheme) -> XDialogBuilder
    {
        self.theme = theme;
        self
    }

    /// Run the XDialog library with the specified configuration. This function will block the main
    /// thread and run the specified `main` function in a separate thread.
    pub fn run(self, main: fn() -> i32) -> i32
    {
        let (send_message, receive_message) = channel::<DialogMessageRequest>();
        init_sender(send_message);

        match self.backend {
            XDialogBackend::Automatic => {
                backends::fltk::FltkBackend::run(main, receive_message, self.theme)
            }
            XDialogBackend::Fltk => {
                backends::fltk::FltkBackend::run(main, receive_message, self.theme)
            }
        }
    }
}

/// Set the silent mode for the dialog. When silent mode is enabled, all dialog functions will
/// return `XDialogResult::SilentMode` without showing any dialogs.
pub fn set_silent_mode(silent: bool) {
    set_silent(silent);
}
