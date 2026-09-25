#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
/// The theme to use for the dialog. The concrete colors and fonts are chosen by each backend;
/// this only selects light vs dark. `SystemDefault` follows the OS/desktop preference where the
/// backend can detect it, otherwise falls back to a light theme.
pub enum XDialogTheme {
    /// Follow the OS/desktop light-or-dark preference (falls back to light if unknown)
    #[default]
    SystemDefault,
    /// Force the backend's light theme
    Light,
    /// Force the backend's dark theme
    Dark,
}

/// Which dialog implementation to use. Chosen at runtime ([`XDialogBuilder::with_backend`]);
/// every variant exists on every platform, and one that can't run on the current platform (or in
/// host mode) makes dialog functions return [`XDialogError::NoBackendAvailable`].
///
/// [`XDialogBuilder::with_backend`]: crate::XDialogBuilder::with_backend
/// [`XDialogError::NoBackendAvailable`]: crate::XDialogError::NoBackendAvailable
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum XDialogBackend {
    /// The platform default (see [Backends](crate#backends)).
    #[default]
    Auto,
    /// Win32 TaskDialog (Windows only).
    Win32,
    /// The WinUI 3 (ContentDialog) look, drawn with egui.
    Fluent,
    /// The classic xdialog Linux look (Ubuntu font), drawn with egui.
    Ubuntu,
    /// Native AppKit (macOS only; not available in host mode, where `Auto` on macOS uses `Ubuntu`).
    AppKit,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
/// The icon to display in the dialog, or None for no icon.
pub enum XDialogIcon {
    /// No icon
    #[default]
    None,
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
    /// except with Win32 TaskDialog, which shows a default button.
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
pub(crate) type CreationSender = std::sync::mpsc::Sender<Result<std::sync::mpsc::Receiver<XDialogResult>, crate::XDialogError>>;

pub(crate) enum DialogMessageRequest {
    // generic
    /// Wake the backend (fonts or the system appearance changed: refresh open dialogs).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))] // sent by Linux background threads only
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
