//! Test hooks.
//!
//! - Environment hooks: `XDIALOG_TEST_NO_ACTIVATE` (all builds); `XDIALOG_TEST_POS` and
//!   `XDIALOG_TEST_ACCENT` (only with `debug_assertions` or `_test-hooks`).
//! - `api` (cfg `xd_test_hooks`): the hidden `xdialog::__test` surface.
//!
//! Live dialogs are published by the manager of every loop
//! (own loop and host mode) in `manager::live`; commands are routed to the owning loop thread.

/// `XDIALOG_TEST_NO_ACTIVATE=1`: never activate/focus dialog windows. Honoured in all builds.
pub(crate) fn no_activate() -> bool {
    std::env::var_os("XDIALOG_TEST_NO_ACTIVATE").is_some_and(|v| !v.is_empty() && v != "0")
}

/// Whether test-only env vars (`XDIALOG_TEST_POS`, `XDIALOG_TEST_ACCENT`) are honoured.
pub(crate) const fn test_env_enabled() -> bool {
    cfg!(any(debug_assertions, xd_test_hooks))
}

/// The hidden `xdialog::__test` API.
#[cfg(xd_test_hooks)]
pub mod api {
    pub use crate::backends::egui_core::offscreen::OffscreenDialog;
    pub use crate::backends::host_types::{HostEvent, Key, Modifiers, MouseButton, ScrollDelta, TouchPhase};

    use crate::backends::egui_core::manager::live::{self, RemoteCmd};

    /// Which kind of dialog to build offscreen.
    #[derive(Clone, Debug)]
    pub enum TestKind {
        /// A message dialog.
        Message,
        /// A progress dialog.
        Progress,
    }

    /// Progress state to apply.
    #[derive(Clone, Debug)]
    pub enum TestProgress {
        /// Determinate value in 0..=1.
        Value(f32),
        /// Indeterminate mode.
        Indeterminate,
    }

    /// Injected appearance (no registry/portal access in offscreen mode).
    #[derive(Clone, Debug, Default)]
    pub struct TestAppearance {
        /// Dark scheme.
        pub dark: bool,
        /// Windows accent palette `[L3, L2, L1, A, D1, D2, D3]` (RGB).
        pub accent_palette: Option<[[u8; 3]; 7]>,
        /// Accent base colour (RGB).
        pub accent: Option<[u8; 3]>,
    }

    /// A live dialog window (own-loop or host mode).
    #[derive(Clone, Debug)]
    pub struct LiveDialog {
        /// Dialog id.
        pub id: usize,
        /// Window title.
        pub title: String,
        /// HWND on Windows, XID on X11, 0 otherwise.
        pub raw_window: isize,
        /// Client size in physical px.
        pub size_px: (u32, u32),
        /// Pixels per point.
        pub ppp: f32,
        /// Button rects in physical px `[x, y, w, h]`, by API index.
        pub button_rects_px: Vec<[f32; 4]>,
        /// Frames presented so far.
        pub frames: u64,
    }

    impl LiveDialog {
        /// Centre of button `i` in physical px, `None` for an unknown index.
        pub fn button_centre(&self, i: usize) -> Option<(f64, f64)> {
            self.button_rects_px.get(i).map(|r| rect_centre(*r))
        }
    }

    /// Centre of an `[x, y, w, h]` rect.
    pub(crate) fn rect_centre(r: [f32; 4]) -> (f64, f64) {
        ((r[0] + r[2] / 2.0) as f64, (r[1] + r[3] / 2.0) as f64)
    }

    /// All live dialogs in this process (own-loop and host-mode windows), by id. A dialog appears
    /// after its first frame was presented and disappears when its window is destroyed; `frames`
    /// counts presented frames (poll it to wait for the effect of an [`inject`]).
    pub fn live_dialogs() -> Vec<LiveDialog> {
        live::snapshot()
    }

    /// Inject an event (physical px coordinates) into a live dialog, routed through its owning
    /// loop (applied on the loop thread, followed by a frame). Ignored (with a warning) for an
    /// unknown or closed dialog.
    pub fn inject(dialog_id: usize, ev: HostEvent) {
        if !live::send(RemoteCmd::Inject(dialog_id, ev)) {
            warn!("xdialog::__test::inject: no live dialog {dialog_id}");
        }
    }
}
