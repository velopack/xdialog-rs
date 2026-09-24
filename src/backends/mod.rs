use crate::model::DialogMessageRequest;
use crate::XDialogError;
use std::sync::mpsc::Receiver;

pub mod select;

#[cfg(windows)]
pub mod win32;

#[cfg(all(windows, feature = "win32-direct"))]
pub mod win32_direct;

#[cfg(target_os = "macos")]
pub mod appkit;

#[cfg(all(target_os = "macos", feature = "maccf-direct"))]
pub mod maccf_direct;

/// Version-neutral window/input types shared by `xdialog::host`, `xdialog::__test` and the macOS
/// host stub. No egui or winit imports.
/// Public only through `xdialog::host` / `xdialog::__test`; otherwise used internally by the egui
/// core's input translation, hence the dead-code allowance for builds without those modules.
#[cfg(any(xd_egui, feature = "winit-host"))]
#[allow(dead_code)]
pub mod host_types;

/// The reusable egui backend core (event loop, windows, rendering, input, animation, ...).
#[cfg(xd_egui)]
pub mod egui_core;

/// The Linux look (the former skia backend's design) on egui.
#[cfg(xd_theme_linux)]
pub mod linux_egui;

/// The Fluent look (WinUI 3 ContentDialog) on egui.
#[cfg(xd_theme_fluent)]
pub mod fluent_egui;

/// Answer every request on `receiver` with an error until `ExitEventLoop` arrives or the channel
/// closes: creation requests get `Err(make_err())`, updates are ignored. Used when no backend can
/// run (no display server, no built-in winit, a failed or panicked event loop).
#[allow(dead_code)]
pub fn drain_with_error(receiver: Receiver<DialogMessageRequest>, make_err: impl Fn() -> XDialogError) {
    while let Ok(message) = receiver.recv() {
        if !answer_with_error(message, &make_err) {
            return;
        }
    }
}

/// Answer one request with an error (see [`drain_with_error`]). Returns `false` for
/// `ExitEventLoop`, `true` otherwise.
#[allow(dead_code)]
pub fn answer_with_error(message: DialogMessageRequest, make_err: &impl Fn() -> XDialogError) -> bool {
    match message {
        DialogMessageRequest::ExitEventLoop => return false,
        DialogMessageRequest::ShowMessageWindow(_, _, creation) => {
            let _ = creation.send(Err(make_err()));
        }
        DialogMessageRequest::ShowProgressWindow(_, _, creation, _) => {
            let _ = creation.send(Err(make_err()));
        }
        DialogMessageRequest::None
        | DialogMessageRequest::CloseWindow(_)
        | DialogMessageRequest::SetProgressIndeterminate(_)
        | DialogMessageRequest::SetProgressValue(_, _)
        | DialogMessageRequest::SetProgressText(_, _) => {}
    }
    true
}
