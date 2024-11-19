use crate::*;

/// Shows a progress dialog with the specified options and returns a proxy object to control it.
/// This is a non-blocking function which will return as soon as the dialog opens.
/// The proxy object can be used to update the progress value, text, or close the dialog.
/// The progress bar can be set to a specific value, or set to indeterminate mode.
///
/// ### Example
/// ```rust
/// use xdialog::*;
///
/// fn main() {
///   XDialogBuilder::new().run(run);
/// }
///
/// fn run() -> i32 {
///   let progress = show_progress(
///     "Window Title",
///     "Main Instruction Text",
///     "Body Text",
///      XDialogIcon::Information).unwrap();
///
///   progress.set_value(0.5).unwrap();
///   progress.set_text("Updating...").unwrap();
///   std::thread::sleep(std::time::Duration::from_secs(3));
///
///   progress.set_indeterminate().unwrap();
///   progress.set_text("Processing...").unwrap();
///   std::thread::sleep(std::time::Duration::from_secs(3));
///
///   progress.close().unwrap();
///   0
/// }
pub fn show_progress<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    title: P1,
    main_instruction: P2,
    message: P3,
    icon: XDialogIcon,
) -> Result<ProgressDialogProxy, XDialogError> {
    let id = get_next_id();

    let data = XDialogOptions {
        title: title.as_ref().to_string(),
        main_instruction: main_instruction.as_ref().to_string(),
        message: message.as_ref().to_string(),
        icon,
        buttons: vec![],
    };

    send_request(DialogMessageRequest::ShowProgressWindow(id, data))?;
    Ok(ProgressDialogProxy { id })
}

/// A proxy object to control a progress dialog. See `show_progress` for more information.
pub struct ProgressDialogProxy {
    id: usize,
}

impl ProgressDialogProxy {
    /// Sets the progress bar to indeterminate mode.
    pub fn set_indeterminate(&self) -> Result<(), XDialogError> {
        send_request(DialogMessageRequest::SetProgressIndeterminate(self.id))
    }

    /// Sets the progress bar to a specific value between 0.0 and 1.0.
    pub fn set_value(&self, value: f32) -> Result<(), XDialogError> {
        send_request(DialogMessageRequest::SetProgressValue(self.id, value))
    }

    /// Sets the text displayed below the progress bar.
    pub fn set_text<P: AsRef<str>>(&self, text: P) -> Result<(), XDialogError> {
        send_request(DialogMessageRequest::SetProgressText(self.id, text.as_ref().to_string()))
    }

    /// Closes the progress dialog.
    pub fn close(&self) -> Result<(), XDialogError> {
        send_request(DialogMessageRequest::CloseWindow(self.id))
    }
}
