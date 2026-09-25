use crate::model::*;

#[derive(Debug, Default)]
/// Builder pattern to configure/initialise the XDialog library. Must be configured and `run` in
/// the main thread before any other XDialog functions are called.
pub struct XDialogBuilder {
    theme: XDialogTheme,
    backend: XDialogBackend,
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

    /// Choose the dialog backend (default [`XDialogBackend::Auto`]).
    pub fn with_backend(mut self, backend: XDialogBackend) -> XDialogBuilder {
        self.backend = backend;
        self
    }

    /// Run with no return value. This is the simplest way to use xdialog when your application
    /// logic does not need to return an exit code or result.
    /// See [`run_loop`](Self::run_loop).
    pub fn run(self, main: fn()) {
        self.run_loop(main);
    }

    /// Run and return an `i32` exit code. This is useful for applications that want to return
    /// a process exit code from their main function.
    /// See [`run_loop`](Self::run_loop).
    pub fn run_i32(self, main: fn() -> i32) -> i32 {
        self.run_loop(main)
    }

    /// Run and return a `Result`. This is useful for applications that use `Result`-based error
    /// handling in their main function.
    /// See [`run_loop`](Self::run_loop).
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
        crate::backends::run_builder(self.backend, self.theme, main)
    }

    /// Wrap your winit 0.30 `ApplicationHandler` so it shows xdialog's dialogs, for an application
    /// that runs its own event loop instead of xdialog's ([`run`](Self::run) and friends). Pass the
    /// returned [`XDialogApp`](crate::host::XDialogApp) to `run_app`; your handler needs no xdialog
    /// code (see the [`host`](crate::host) module). Call once, on the event-loop thread, before any
    /// dialog function.
    ///
    /// `waker` is called from any thread when xdialog needs an event-loop iteration. It must make
    /// the loop iterate (typically `let _ = proxy.send_event(MyEvent::XDialog)`; your `user_event`
    /// ignores it), must not block, and may be called redundantly (xdialog coalesces: at most one
    /// outstanding call per `about_to_wait`).
    ///
    /// Errors: `SystemError` if a dialog backend was already initialized (another
    /// `XDialogBuilder`, `init_*`, or `into_host_app`); `NoBackendAvailable` if the chosen backend
    /// can't run in host mode (`Win32` only on Windows, never `AppKit`; `Auto` always can). On
    /// error nothing is installed and `app` is dropped.
    #[cfg(feature = "winit-host")]
    pub fn into_host_app<A>(self, app: A, waker: impl Fn() + Send + 'static) -> Result<crate::host::XDialogApp<A>, crate::XDialogError> {
        crate::host::XDialogApp::new(app, self.backend, self.theme, Box::new(waker))
    }
}
