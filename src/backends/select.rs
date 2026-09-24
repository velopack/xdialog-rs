//! Chooses the backend used by `XDialogBuilder`.
//!
//! `XDIALOG_BACKEND=win32|fluent|linux` (hidden, read once) chooses among the backends compiled
//! into this build. Unknown or uncompiled values log a warning and use the default. In a default
//! build there is only one option per OS, so it does nothing.

use std::sync::OnceLock;

/// A backend `XDialogBuilder` can run.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    /// Win32 TaskDialog (Windows default).
    #[cfg(windows)]
    Win32,
    /// AppKit (macOS).
    #[cfg(target_os = "macos")]
    AppKit,
    /// egui with the Linux (skia-look) theme on xdialog's own winit loop.
    #[cfg(all(xd_own_loop, xd_theme_linux))]
    LinuxEgui,
    /// egui with the Fluent theme on xdialog's own winit loop.
    #[cfg(all(xd_own_loop, xd_theme_fluent))]
    FluentEgui,
    /// No builder backend is compiled (Linux built without `builtin-winit`): dialog functions
    /// return `NoBackendAvailable` unless a host handler (`init_winit_host`) is installed.
    None,
}

impl BackendKind {
    /// Stable id used by `XDIALOG_BACKEND` and the harnesses.
    #[allow(dead_code)]
    pub fn id(self) -> &'static str {
        match self {
            #[cfg(windows)]
            BackendKind::Win32 => "win32",
            #[cfg(target_os = "macos")]
            BackendKind::AppKit => "appkit",
            #[cfg(all(xd_own_loop, xd_theme_linux))]
            BackendKind::LinuxEgui => "linux",
            #[cfg(all(xd_own_loop, xd_theme_fluent))]
            BackendKind::FluentEgui => "fluent",
            BackendKind::None => "none",
        }
    }
}

/// Every builder backend compiled into this build, default first.
pub fn compiled_backends() -> Vec<BackendKind> {
    #[allow(unused_mut)]
    let mut v: Vec<BackendKind> = Vec::new();

    // Default first.
    #[cfg(all(windows, xd_own_loop, xd_theme_fluent))]
    v.push(BackendKind::FluentEgui);
    #[cfg(windows)]
    v.push(BackendKind::Win32);
    #[cfg(all(windows, xd_own_loop, xd_theme_linux))]
    v.push(BackendKind::LinuxEgui);

    #[cfg(target_os = "macos")]
    v.push(BackendKind::AppKit);

    #[cfg(all(target_os = "linux", xd_own_loop, xd_theme_linux))]
    v.push(BackendKind::LinuxEgui);
    #[cfg(all(target_os = "linux", xd_own_loop, xd_theme_fluent))]
    v.push(BackendKind::FluentEgui);

    if v.is_empty() {
        v.push(BackendKind::None);
    }
    v
}

/// The backend `XDialogBuilder` uses: `XDIALOG_BACKEND` if it names a compiled backend, else the
/// default (the first of [`compiled_backends`]). Computed once per process.
pub fn builder_backend() -> BackendKind {
    static CHOSEN: OnceLock<BackendKind> = OnceLock::new();
    *CHOSEN.get_or_init(|| {
               let compiled = compiled_backends();
               let default = compiled[0];
               match std::env::var("XDIALOG_BACKEND") {
                   Ok(value) if !value.trim().is_empty() => {
                       let want = value.trim().to_ascii_lowercase();
                       match compiled.iter().copied().find(|k| k.id() == want) {
                           Some(kind) => kind,
                           None => {
                               warn!("xdialog: XDIALOG_BACKEND={value} is unknown or not compiled into this build (available: {}); using \
                                      {}",
                                     compiled.iter().map(|k| k.id()).collect::<Vec<_>>().join(", "),
                                     default.id());
                               default
                           }
                       }
                   }
                   _ => default,
               }
           })
}
