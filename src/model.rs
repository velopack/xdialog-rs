#[derive(Debug, Clone, Eq, PartialEq)]
/// The backend to use for the dialog. Automatic will choose the best backend for the current platform.
pub enum XDialogBackend {
    /// Automatically choose the best backend for the current platform
    Automatic = 0,
    /// Use the FLTK backend
    Fltk,
    /// Prefer the native backend for the current platform (eg. Win32), falling back to FLTK
    NativePreferred,
    /// Use the GTK3 backend (Linux only)
    Gtk,
    /// Use the pure-Rust software renderer (winit + tiny-skia). Available on all platforms; the
    /// default on Linux. Selectable elsewhere for testing or to avoid native dialogs.
    Skia,
}

#[derive(Debug, Clone, Eq, PartialEq)]
/// The theme to use for the dialog. The concrete colors and fonts are chosen by each backend;
/// this only selects light vs dark. `SystemDefault` follows the OS/desktop preference where the
/// backend can detect it, otherwise falls back to a light theme.
pub enum XDialogTheme {
    /// Follow the OS/desktop light-or-dark preference (falls back to light if unknown)
    SystemDefault = 0,
    /// Force the backend's light theme
    Light,
    /// Force the backend's dark theme
    Dark,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
/// The icon to display in the dialog, or None for no icon.
pub enum XDialogIcon {
    /// No icon
    #[default]
    None = 0,
    /// Error icon
    Error,
    /// Warning icon
    Warning,
    /// Information icon
    Information,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
/// Options for constructing a new custom message or progress dialog
pub struct XDialogOptions {
    /// The title of the dialog window (required)
    pub title: String,
    /// The main instruction / header text. Can be set to an empty string to hide this element.
    pub main_instruction: String,
    /// The body text of the dialog. Can be set to an empty string to hide this element.
    pub message: String,
    /// The icon to display in the dialog, or None for no icon.
    pub icon: XDialogIcon,
    /// The buttons to display in the dialog. This can be an empty array to collapse the button panel.
    /// For progress dialogs the buttons are shown on every platform; an empty array shows no button
    /// except on platforms which require one (Windows shows a default button).
    pub buttons: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
/// The result of a blocking dialog operation
pub enum XDialogResult {
    /// The dialog was closed without a button being pressed (eg. user clicked 'X' button)
    WindowClosed,
    /// The dialog was closed because the timeout elapsed
    TimeoutElapsed,
    /// The dialog was not shown because silent mode is currently enabled
    SilentMode,
    /// A button was pressed, with the index of the button in the `buttons` array
    ButtonPressed(usize),
}

/// Channel sender used by backends to deliver the dialog result receiver back to the caller.
/// Sends `Ok(receiver)` on successful dialog creation, or `Err(e)` on failure.
pub type CreationSender = oneshot::Sender<Result<oneshot::Receiver<XDialogResult>, crate::XDialogError>>;

#[allow(missing_docs)]
#[derive(Debug, Default)]
pub enum DialogMessageRequest {
    // generic
    #[default]
    None,
    ExitEventLoop,
    CloseWindow(usize),

    // messagebox
    ShowMessageWindow(usize, XDialogOptions, CreationSender),

    // progress
    ShowProgressWindow(usize, XDialogOptions, CreationSender, Option<crate::progress::ProgressButtonCallback>),
    SetProgressIndeterminate(usize),
    SetProgressValue(usize, f32),
    SetProgressText(usize, String),
}
