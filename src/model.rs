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

/// The one reply a backend owes the caller of a show request (see [`crate::oneshot`]): exactly one
/// of [`opened`](Self::opened) or [`failed`](Self::failed). Dropped unanswered, the caller gets
/// `NoResult`.
pub(crate) enum DialogReply {
    /// A message box: its result, or the error that kept it from opening.
    Message(crate::oneshot::Sender<Result<XDialogResult, crate::XDialogError>>),
    /// A progress dialog: whether it opened (its caller waits only for that).
    Progress(crate::oneshot::Sender<Result<(), crate::XDialogError>>),
}

impl DialogReply {
    /// The dialog is open: a progress caller is told now, a message box's result follows through
    /// the returned sender.
    pub(crate) fn opened(self) -> ResultSender {
        match self {
            DialogReply::Message(tx) => ResultSender(Some(tx)),
            DialogReply::Progress(tx) => {
                let _ = tx.send(Ok(()));
                ResultSender(None)
            }
        }
    }

    /// The dialog could not be shown.
    pub(crate) fn failed(self, e: crate::XDialogError) {
        match self {
            DialogReply::Message(tx) => {
                let _ = tx.send(Err(e));
            }
            DialogReply::Progress(tx) => {
                let _ = tx.send(Err(e));
            }
        }
    }
}

/// Delivers an open dialog's result: a message box's to its caller, a progress dialog's nowhere.
/// The first result wins.
pub(crate) struct ResultSender(Option<crate::oneshot::Sender<Result<XDialogResult, crate::XDialogError>>>);

impl ResultSender {
    pub(crate) fn send(&self, result: XDialogResult) {
        if let Some(tx) = &self.0 {
            let _ = tx.send(Ok(result));
        }
    }
}

pub(crate) enum DialogMessageRequest {
    // generic
    /// Wake the backend (fonts or the system appearance changed: refresh open dialogs).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))] // sent by Linux background threads only
    None,
    ExitEventLoop,
    CloseWindow(usize),

    // messagebox
    ShowMessageWindow(usize, XDialogOptions, DialogReply),

    // progress
    ShowProgressWindow(usize, XDialogOptions, DialogReply, Option<crate::progress::ProgressButtonCallback>),
    SetProgressIndeterminate(usize),
    SetProgressValue(usize, f32),
    SetProgressText(usize, String),
}
