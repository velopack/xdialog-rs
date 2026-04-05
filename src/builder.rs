use std::{sync::mpsc::channel, thread};

use crate::backends::XDialogBackendImpl;
use crate::channel::{send_request, ChannelHandler};
use crate::model::*;

#[derive(Debug)]
/// Builder pattern to configure/initialise the XDialog library. Must be configured and `run` in
/// the main thread before any other XDialog functions are called.
pub struct XDialogBuilder {
    backend: XDialogBackend,
    theme: XDialogTheme,
}

impl Default for XDialogBuilder {
    fn default() -> XDialogBuilder {
        XDialogBuilder { backend: XDialogBackend::Automatic, theme: XDialogTheme::SystemDefault }
    }
}

impl XDialogBuilder {
    /// Create a new XDialogBuilder
    pub fn new() -> XDialogBuilder {
        XDialogBuilder::default()
    }

    /// Set the backend to use for the dialog. By default, the backend is chosen automatically.
    pub fn with_backend(mut self, backend: XDialogBackend) -> XDialogBuilder {
        self.backend = backend;
        self
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
        crate::channel::init_handler(Box::new(ChannelHandler { sender: send_message }));

        let result = thread::spawn(move || {
            let result = main();
            let _ = send_request(DialogMessageRequest::ExitEventLoop);
            result
        });

        let backend_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Self::dispatch_backend(self.backend, receive_message, self.theme);
        }));

        if let Err(e) = backend_result {
            error!("xdialog: backend panicked: {:?}", e);
        }

        match result.join() {
            Ok(val) => val,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

impl XDialogBuilder {
    #[cfg(windows)]
    fn run_default_backend(receiver: std::sync::mpsc::Receiver<DialogMessageRequest>, theme: XDialogTheme) {
        crate::backends::win32::Win32Backend::run_loop(receiver, theme);
    }

    #[cfg(target_os = "macos")]
    fn run_default_backend(receiver: std::sync::mpsc::Receiver<DialogMessageRequest>, theme: XDialogTheme) {
        crate::backends::appkit::AppKitBackend::run_loop(receiver, theme);
    }

    #[cfg(target_os = "linux")]
    fn run_default_backend(receiver: std::sync::mpsc::Receiver<DialogMessageRequest>, theme: XDialogTheme) {
        crate::backends::gtk3::GtkBackend::run_loop(receiver, theme);
    }

    fn dispatch_backend(
        backend: XDialogBackend,
        receiver: std::sync::mpsc::Receiver<DialogMessageRequest>,
        theme: XDialogTheme,
    ) {
        match backend {
            XDialogBackend::Automatic | XDialogBackend::NativePreferred => {
                Self::run_default_backend(receiver, theme);
            }
            #[cfg(target_os = "linux")]
            XDialogBackend::Fltk => crate::backends::fltk::FltkBackend::run_loop(receiver, theme),
            #[cfg(not(target_os = "linux"))]
            XDialogBackend::Fltk => {
                error!("xdialog: FLTK backend is only available on Linux");
                Self::dispatch_backend(XDialogBackend::Automatic, receiver, theme);
            }
            #[cfg(target_os = "linux")]
            XDialogBackend::Gtk => crate::backends::gtk3::GtkBackend::run_loop(receiver, theme),
            #[cfg(not(target_os = "linux"))]
            XDialogBackend::Gtk => {
                error!("xdialog: GTK backend is only available on Linux");
                Self::dispatch_backend(XDialogBackend::Automatic, receiver, theme);
            }
        }
    }
}
