#[derive(Debug, Clone, Eq, PartialEq)]
/// The backend to use for the dialog. Automatic will choose the best backend for the current platform.
pub enum XDialogBackend {
    /// Automatically choose the best backend for the current platform
    Automatic = 0,
    /// Use the FLTK backend
    Fltk,
    /// Use Windows-only XAML Islands
    XamlIsland,
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

#[derive(Debug, Clone, Eq, PartialEq)]
/// The icon to display in the dialog, or None for no icon.
pub enum XDialogIcon {
    /// No icon
    None = 0,
    /// Error icon
    Error,
    /// Warning icon
    Warning,
    /// Information icon
    Information,
}

#[derive(Debug, Clone, Eq, PartialEq)]
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
    /// The dialog was not shown because silent mode is currently enabled
    SilentMode,
    /// A button was pressed, with the index of the button in the `buttons` array
    ButtonPressed(usize),
}

#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq)]
pub enum DialogMessageRequest {
    None,
    ShowMessageWindow(usize, XDialogOptions),
    ShowProgressWindow(usize, XDialogOptions),
    CloseWindow(usize),
    SetProgressIndeterminate(usize),
    SetProgressValue(usize, f32),
    SetProgressText(usize, String),
    ExitEventLoop,
}
