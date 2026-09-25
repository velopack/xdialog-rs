#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[macro_use]
extern crate log;

pub use message::*;
pub use model::*;
pub use progress::*;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

mod backends;

mod channel;
use channel::send_request;
mod builder;
pub use builder::*;

#[cfg(all(windows, feature = "win32-direct"))]
pub use backends::win32::init_win32_direct;

#[cfg(all(target_os = "macos", feature = "maccf-direct"))]
pub use backends::maccf_direct::init_maccf_direct;

#[cfg(feature = "winit-host")]
pub mod host;

#[cfg(feature = "_test-hooks")]
#[doc(hidden)]
pub mod __test {
    //! Test-only hooks (offscreen rendering). Not part of the public API.
    pub use crate::backends::egui_core::offscreen::{OffscreenDialog, TestAppearance, TestProgress};
    pub use crate::backends::egui_core::theme::DialogKind as TestKind;
    pub use egui;
}

mod message;
mod model;
mod progress;

static SILENT: AtomicBool = AtomicBool::new(false);
static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

/// Set the silent mode for the dialog. When silent mode is enabled, all dialog functions will
/// return `XDialogResult::SilentMode` without showing any dialogs.
pub fn set_silent_mode(silent: bool) {
    SILENT.store(silent, Ordering::Relaxed);
}

fn get_silent() -> bool {
    SILENT.load(Ordering::Relaxed)
}

/// A new dialog id.
fn get_next_id() -> usize {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

#[allow(missing_docs)]
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum XDialogError {
    #[error("xdialog backend not initialized")]
    NotInitialized,
    #[error("xdialog command returned no result: {0}")]
    NoResult(std::sync::mpsc::RecvError),
    /// The backend runs but failed, for example a dialog's window or surface couldn't be created.
    #[error("xdialog generic error: {0}")]
    SystemError(String),
    /// No backend can show dialogs: no display server (X11 or Wayland) on Linux, the chosen
    /// [`XDialogBackend`] can't run on this platform or in host mode, or the host app
    /// (`XDialogApp`) exited / the builder's event loop ended.
    #[error("no xdialog backend available (no display server, backend not available here, or dialogs shut down)")]
    NoBackendAvailable,
    /// A blocking dialog function (such as `show_message`) was called on xdialog's UI thread,
    /// where waiting would deadlock; call it from another thread (see [Threads](crate#threads)).
    #[error("xdialog: blocking dialog call on the xdialog UI thread (dialog callback or host event-loop thread) would deadlock")]
    BlockingCallOnUiThread,
}
