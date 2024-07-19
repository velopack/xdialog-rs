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
    MacOS,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum MessageBoxIcon {
    None = 0,
    Error,
    Warning,
    Information,
    Question,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MessageBoxData {
    pub title: String,
    pub main_instruction: String,
    pub message: String,
    pub icon: MessageBoxIcon,
    pub buttons: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum MessageBoxResult {
    WindowClosed,
    SilentMode,
    ButtonPressed(usize),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DialogMessageRequest {
    None,
    ShowMessageBox(usize, MessageBoxData),
    ShowProgressDialog(usize),
    SetProgressDialogIndeterminate(usize),
    SetProgressDialogValue(usize, usize),
    SetProgressDialogText(usize, String),
    ExitEventLoop,
}