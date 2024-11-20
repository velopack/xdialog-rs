use crate::*;

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
/// Describes the state of a webview dialog window
pub enum XDialogWindowState {
    /// The window is in a normal state
    #[default]
    Normal,
    /// The window is hidden from view (not visible in the taskbar)
    Hidden,
    /// The window is minimized to the taskbar
    Minimized,
    /// The window is maximized to fill the screen
    Maximized,
    /// The window is in fullscreen mode
    Fullscreen,
}

#[derive(Debug, Clone, PartialEq, Default)]
/// Options for constructing a new webview dialog
pub struct XDialogWebviewOptions {
    /// The title of the dialog window (required)
    pub title: String,
    /// The initial HTML content to display in the webview
    pub html: String,
    /// The initial size of the dialog window
    pub size: Option<(i32, i32)>,
    /// If true, the dialog window will be hidden when first created and will need to be shown manually
    pub hidden: bool,
    /// The minimum size of the dialog window
    pub min_size: Option<(i32, i32)>,
    /// If true, the dialog window can NOT be resized by the user
    pub fixed_size: bool,
    /// If true, the dialog window will have a borderless frame
    pub borderless: bool,
    /// A callback function which is executed when a javascript message is dispatched with `window.external.invoke(message)`
    pub callback: Option<WebviewInvokeHandler>,
}

#[allow(missing_docs)]
#[derive(Debug)]
pub struct ResultSender {
    pub sender: Option<oneshot::Sender<Result<(), XDialogError>>>,
}

#[allow(missing_docs)]
impl ResultSender {
    pub fn send_result(&mut self, result: Result<(), XDialogError>) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(result);
        }
    }
    pub fn create() -> (Self, oneshot::Receiver<Result<(), XDialogError>>) {
        let (sender, receiver) = oneshot::channel();
        let result_sender = ResultSender { sender: Some(sender) };
        (result_sender, receiver)
    }
}

impl PartialEq for ResultSender {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// A callback function which is executed when a javascript message is dispatched with
/// `window.external.invoke(message)`. The `WebviewDialogProxy` provided in this callback
/// is not the same as the one returned by `show_webview`, but it can be used to interact
/// with the webview in the same way.
pub type WebviewInvokeHandler = fn(webview: crate::WebviewDialogProxy, message: String);

#[allow(missing_docs)]
#[derive(Debug, PartialEq)]
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
    WebviewWindowShow(usize, XDialogWebviewOptions, ResultSender),
    WebviewSetTitle(usize, String, ResultSender),
    WebviewSetHtml(usize, String, ResultSender),
    WebviewSetPosition(usize, i32, i32, ResultSender),
    WebviewSetSize(usize, i32, i32, ResultSender),
    WebviewSetZoomLevel(usize, f64, ResultSender),
    WebviewSetWindowState(usize, XDialogWindowState, ResultSender),
    WebviewEval(usize, String, ResultSender),
}
