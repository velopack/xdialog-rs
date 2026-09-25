use std::sync::mpsc::{channel, Receiver};
use std::thread;

use crate::channel::{send_request, ChannelHandler};
use crate::model::*;

#[derive(Debug)]
/// Builder pattern to configure/initialise the XDialog library. Must be configured and `run` in
/// the main thread before any other XDialog functions are called.
pub struct XDialogBuilder {
    theme: XDialogTheme,
}

impl Default for XDialogBuilder {
    fn default() -> XDialogBuilder {
        XDialogBuilder { theme: XDialogTheme::SystemDefault }
    }
}

impl XDialogBuilder {
    /// Create a new XDialogBuilder
    pub fn new() -> XDialogBuilder {
        XDialogBuilder::default()
    }

    /// Set the theme to use for the dialog. By default, the theme is chosen automatically.
    pub fn with_theme(mut self, theme: XDialogTheme) -> XDialogBuilder {
        self.theme = theme;
        self
    }

    /// Run with no return value. This is the simplest way to use xdialog when your application
    /// logic does not need to return an exit code or result.
    ///
    /// This function will block the main thread and run the specified `main` function in a
    /// separate thread.
    pub fn run(self, main: fn()) {
        self.run_loop(main);
    }

    /// Run and return an `i32` exit code. This is useful for applications that want to return
    /// a process exit code from their main function.
    ///
    /// This function will block the main thread and run the specified `main` function in a
    /// separate thread.
    pub fn run_i32(self, main: fn() -> i32) -> i32 {
        self.run_loop(main)
    }

    /// Run and return a `Result`. This is useful for applications that use `Result`-based error
    /// handling in their main function.
    ///
    /// This function will block the main thread and run the specified `main` function in a
    /// separate thread.
    pub fn run_result<T: Send + 'static, E: Send + 'static>(self, main: fn() -> Result<T, E>) -> Result<T, E> {
        self.run_loop(main)
    }

    /// Run the XDialog library with the specified configuration, returning an arbitrary type.
    /// For most use cases, prefer [`run`](Self::run), [`run_i32`](Self::run_i32), or
    /// [`run_result`](Self::run_result) instead.
    ///
    /// This function will block the main thread and run the specified `main` function in a
    /// separate thread.
    pub fn run_loop<T: Send + 'static>(self, main: fn() -> T) -> T {
        let (send_message, receive_message) = channel::<DialogMessageRequest>();
        let installed = crate::channel::init_handler(Box::new(ChannelHandler { sender: send_message }));

        let result = thread::spawn(move || {
            let result = main();
            // Only our own backend is stopped: a handler installed earlier (`init_linux_direct`,
            // `init_winit_host`, ...) belongs to someone else, and `ExitEventLoop` would close its
            // dialogs (or start linux-direct's UI thread just to handle it).
            if installed {
                let _ = send_request(DialogMessageRequest::ExitEventLoop);
            }
            result
        });

        if installed {
            let backend_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                                              Self::run_default_backend(receive_message, self.theme);
                                                          }));

            if let Err(e) = backend_result {
                error!("xdialog: backend panicked: {:?}", e);
            }
        } else {
            // A handler was already installed (init_winit_host / init_linux_direct /
            // init_win32_direct ran first): requests go to that handler, so no builder backend is
            // started here (and no `ExitEventLoop` is sent); just wait for `main`.
            warn!("xdialog: a request handler is already installed; XDialogBuilder runs main without its own backend");
            drop(receive_message);
        }

        match result.join() {
            Ok(val) => val,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

impl XDialogBuilder {
    fn run_default_backend(receiver: Receiver<DialogMessageRequest>, theme: XDialogTheme) {
        use crate::backends::select::{builder_backend, BackendKind};
        match builder_backend() {
            #[cfg(windows)]
            BackendKind::Win32 => crate::backends::win32::Win32Backend::run_loop(receiver, theme),
            #[cfg(target_os = "macos")]
            BackendKind::AppKit => crate::backends::appkit::AppKitBackend::run_loop(receiver, theme),
            #[cfg(all(xd_own_loop, xd_theme_ubuntu))]
            BackendKind::EguiUbuntu => Self::run_egui(crate::backends::egui_ubuntu::UbuntuTheme::new(), receiver, theme),
            #[cfg(all(xd_own_loop, xd_theme_fluent))]
            BackendKind::EguiFluent => Self::run_egui(crate::backends::egui_fluent::FluentTheme::new(), receiver, theme),
            BackendKind::None => {
                let _ = theme;
                crate::backends::drain_with_error(receiver, || crate::XDialogError::NoBackendAvailable);
            }
        }
    }

    /// Run the egui own loop with `theme_impl`. If it could not be built (an `Err` or a panic
    /// inside `EventLoop::build`), nothing has consumed the receiver yet, so fall back cleanly:
    /// Win32 on Windows, `NoBackendAvailable` elsewhere.
    #[cfg(xd_own_loop)]
    fn run_egui<T: crate::backends::egui_core::theme::Theme>(theme_impl: T, receiver: Receiver<DialogMessageRequest>, theme: XDialogTheme) {
        if let Err(failed) = crate::backends::egui_core::own_loop::run_builder(theme_impl, receiver, theme.clone()) {
            warn!("xdialog: egui backend unavailable ({}), falling back", failed.reason);
            #[cfg(windows)]
            crate::backends::win32::Win32Backend::run_loop(failed.receiver, theme);
            #[cfg(not(windows))]
            {
                let _ = theme;
                crate::backends::drain_with_error(failed.receiver, || crate::XDialogError::NoBackendAvailable);
            }
        }
    }
}
