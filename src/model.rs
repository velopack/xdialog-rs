#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
/// Light or dark dialog theme; the concrete colors and fonts are chosen by each backend.
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
    /// The WinUI 3 (ContentDialog) look, drawn by xdialog.
    Fluent,
    /// The classic xdialog Linux look (Ubuntu font), drawn by xdialog.
    Ubuntu,
    /// The macOS alert look (Big Sur to Sequoia), drawn by xdialog.
    MacOS,
    /// Native AppKit (macOS only; not available in host mode).
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
    /// The image of [`XDialogOptions::icon_source`], with the drawn backends. Other backends, or a
    /// missing or unloadable icon source, show no icon.
    Custom,
}

/// An `.ico`, `.png` or `.icns` image (the format is detected from the content), for
/// [`XDialogOptions::icon_source`].
#[derive(Clone, PartialEq, Eq)]
pub enum XDialogIconSource {
    /// A file, read when the dialog is shown.
    File(std::path::PathBuf),
    /// The file's content, e.g. `Bytes(include_bytes!("app.ico").as_slice().into())` or
    /// `Bytes(vec.into())`.
    Bytes(std::borrow::Cow<'static, [u8]>),
}

impl std::fmt::Debug for XDialogIconSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XDialogIconSource::File(path) => f.debug_tuple("File").field(path).finish(),
            XDialogIconSource::Bytes(bytes) => write!(f, "Bytes({} bytes)", bytes.len()),
        }
    }
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
    /// An `.ico`, `.png` or `.icns` image. With the drawn backends (Fluent, Ubuntu, MacOS) it is the
    /// window / taskbar icon where the platform has one (Windows, X11; not Wayland or macOS), and
    /// the icon shown in the dialog with [`XDialogIcon::Custom`]. The other backends ignore it. An
    /// image that can't be read or decoded is logged and ignored. PNG images (a `.png`, or a PNG
    /// frame of an `.ico`) must be 8-bit RGB, RGBA, gray or gray + alpha: palette (indexed) and
    /// 16-bit PNGs are not supported. Frames over 1024 px are skipped.
    pub icon_source: Option<XDialogIconSource>,
    /// The dialog's buttons. Empty hides the button panel, except with Win32 TaskDialog (message or
    /// progress) and a maccf-direct message dialog, which then show an OK button that reports
    /// [`XDialogResult::WindowClosed`].
    pub buttons: Vec<String>,
}

impl XDialogOptions {
    /// The options of the shortcut functions: no icon source.
    pub(crate) fn basic(title: &str, main_instruction: &str, message: &str, icon: XDialogIcon, buttons: &[&str]) -> Self {
        XDialogOptions { title: title.to_string(),
                         main_instruction: main_instruction.to_string(),
                         message: message.to_string(),
                         icon,
                         icon_source: None,
                         buttons: buttons.iter().map(|b| b.to_string()).collect() }
    }
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
    #[cfg_attr(not(draw_soft), allow(dead_code))] // sent by the soft backend's font scan / appearance watcher only
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
