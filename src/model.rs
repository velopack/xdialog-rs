#[derive(Debug, Clone, Eq, PartialEq)]
pub enum XDialogBackend {
    Automatic = 0,
    Fltk,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum XDialogTheme {
    SystemDefault = 0,
    Windows,
    Ubuntu,
    MacOSLight,
    MacOSDark,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum XDialogIcon {
    None = 0,
    Error,
    Warning,
    Information,
    Question,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct XDialogMessageBox {
    pub title: String,
    pub main_instruction: String,
    pub message: String,
    pub icon: XDialogIcon,
    pub buttons: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum XDialogResult {
    WindowClosed,
    SilentMode,
    ButtonPressed(usize),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DialogMessageRequest {
    None,
    ShowMessageBox(usize, XDialogMessageBox),
    ShowProgressDialog(usize),
    SetProgressDialogIndeterminate(usize),
    SetProgressDialogValue(usize, usize),
    SetProgressDialogText(usize, String),
    ExitEventLoop,
}