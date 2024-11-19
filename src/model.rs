use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Eq, PartialEq)]
/// The backend to use for the dialog. Automatic will choose the best backend for the current platform.
pub enum XDialogBackend {
    /// Automatically choose the best backend for the current platform
    Automatic = 0,
    /// Use the FLTK backend
    Fltk,
    /// Use the Native backend for given platform (eg. Win32, GTK, Cocoa)
    Native,
    // Use Windows-only XAML Islands
    // XamlIsland,
}

#[derive(Debug, Clone, Eq, PartialEq)]
/// The theme to use for the dialog. SystemDefault will use the most relevant theme for the current platform.
pub enum XDialogTheme {
    /// Automatically choose the best theme for the current platform
    SystemDefault = 0,
    /// Windows theme
    Windows,
    /// Ubuntu theme
    Ubuntu,
    /// MacOS light theme
    MacOSLight,
    /// MacOS dark theme
    MacOSDark,
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
/// Options for constructing a new custom message dialog
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

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub enum XDialogWindowState {
    #[default]
    Normal,
    Hidden,
    Minimized,
    Maximized,
    Fullscreen,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct XDialogWebviewOptions {
    pub title: String,
    pub html: String,
    // pub position: Option<(i32, i32)>,
    pub size: Option<(i32, i32)>,
    pub hidden: bool,
    pub min_size: Option<(i32, i32)>,
    pub resizable: bool,
    pub borderless: bool,
    pub hide_on_close: bool,
}

pub(crate) enum WebviewResponse {
    ErrorOpening(String)
}

#[derive(Debug, Clone)]
pub(crate) struct WebviewSender {
    pub sender: Sender<String>,
}

impl PartialEq for WebviewSender {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Into<WebviewSender> for Sender<String> {
    fn into(self) -> WebviewSender {
        WebviewSender { sender: self }
    }
}

#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq)]
pub enum DialogMessageRequest {
    // generic
    None,
    ExitEventLoop,
    CloseWindow(usize),
    // SetWindowTitle(usize, String),

    // messagebox
    ShowMessageWindow(usize, XDialogOptions),

    // progress
    ShowProgressWindow(usize, XDialogOptions),
    SetProgressIndeterminate(usize),
    SetProgressValue(usize, f32),
    SetProgressText(usize, String),

    // webview
    ShowWebviewWindow(usize, XDialogWebviewOptions, WebviewSender),
    // SetWebviewHtml(usize, String),
    // SetWebviewPosition(usize, i32, i32),
    // SetWebviewSize(usize, i32, i32),
    // SetWebviewZoomLevel(usize, f64),
    // SetWebviewWindowState(usize, XDialogWindowState),
}
