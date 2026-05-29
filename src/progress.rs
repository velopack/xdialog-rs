use crate::*;

/// Shows a progress dialog with the specified options and returns a proxy object to control it.
/// This is a non-blocking function which will return as soon as the dialog opens.
/// The proxy object can be used to update the progress value, text, or close the dialog.
/// The progress bar can be set to a specific value, or set to indeterminate mode.
///
/// This progress dialog has no buttons. On platforms which require a button to be present
/// (Windows), a default button is shown. See [`show_progress_ex`] to customize the buttons,
/// or [`show_progress_with_callback`] to also react when a button is clicked.
///
/// ### Example
/// ```rust,no_run
/// use xdialog::*;
///
/// fn main() {
///   XDialogBuilder::new().run_i32(run);
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
/// ```
pub fn show_progress<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    title: P1,
    main_instruction: P2,
    message: P3,
    icon: XDialogIcon,
) -> Result<ProgressDialogProxy, XDialogError> {
    let data = XDialogOptions {
        title: title.as_ref().to_string(),
        main_instruction: main_instruction.as_ref().to_string(),
        message: message.as_ref().to_string(),
        icon,
        buttons: vec![],
    };
    show_progress_internal(data, None)
}

/// Shows a progress dialog with custom buttons. Like [`show_progress`], but the buttons in
/// `options` are displayed on every platform. Clicking a button closes the dialog. To also be
/// notified when a button is clicked (eg. to cancel an operation), use
/// [`show_progress_with_callback`].
///
/// This is useful to relabel the button that Windows always displays on a progress dialog (eg.
/// to a localized "Hide"), or to offer a "Cancel" button on all platforms.
pub fn show_progress_ex(options: XDialogOptions) -> Result<ProgressDialogProxy, XDialogError> {
    show_progress_internal(options, None)
}

/// Shows a progress dialog with custom buttons and a callback invoked when a button is clicked.
///
/// The callback runs on the backend thread and receives the index of the clicked button (into
/// `options.buttons`) and a non-owning [`ProgressDialogProxy`] which can be used to update the
/// dialog from within the callback (eg. `proxy.set_text("Cancelling...")`). Dropping the proxy
/// passed to the callback does *not* close the dialog.
///
/// The callback returns a `bool` controlling what happens next: return `true` to keep the dialog
/// open (eg. to show a "Cancelling..." message until your operation finishes), or `false` to
/// close it immediately.
///
/// Note: the callback is never invoked in silent mode. Pair callbacks with a non-empty `buttons`
/// list — with an empty list only Windows shows a (default) button and its index will not map to
/// your `buttons` array.
///
/// ### Example
/// ```rust,no_run
/// use xdialog::*;
/// use std::sync::Arc;
/// use std::sync::atomic::{AtomicBool, Ordering};
///
/// # fn run() {
/// let cancelled = Arc::new(AtomicBool::new(false));
/// let flag = cancelled.clone();
/// let progress = show_progress_with_callback(
///     XDialogOptions {
///         title: "Working".to_string(),
///         main_instruction: "Please wait".to_string(),
///         message: "Crunching numbers...".to_string(),
///         icon: XDialogIcon::Information,
///         buttons: vec!["Cancel".to_string()],
///     },
///     move |_button_index, proxy| {
///         flag.store(true, Ordering::SeqCst);
///         let _ = proxy.set_text("Cancelling...");
///         true // keep the dialog open until we tear down
///     },
/// ).unwrap();
/// # let _ = (cancelled, progress);
/// # }
/// ```
pub fn show_progress_with_callback<F>(options: XDialogOptions, on_button: F) -> Result<ProgressDialogProxy, XDialogError>
where
    F: FnMut(usize, &ProgressDialogProxy) -> bool + Send + 'static,
{
    show_progress_internal(options, Some(ProgressButtonCallback(Box::new(on_button))))
}

fn show_progress_internal(options: XDialogOptions, on_button: Option<ProgressButtonCallback>) -> Result<ProgressDialogProxy, XDialogError> {
    let id = get_next_id();

    if get_silent() {
        return Ok(ProgressDialogProxy { id, silent: true, owned: true });
    }

    let (creation_sender, creation_receiver) = oneshot::channel();
    send_request(DialogMessageRequest::ShowProgressWindow(id, options, creation_sender, on_button))?;
    // Wait for creation confirmation, discard the dialog result receiver
    let _ = creation_receiver.recv().map_err(XDialogError::NoResult)??;
    Ok(ProgressDialogProxy { id, silent: false, owned: true })
}

/// The boxed closure type behind [`ProgressButtonCallback`].
type ProgressButtonCallbackFn = Box<dyn FnMut(usize, &ProgressDialogProxy) -> bool + Send + 'static>;

/// A boxed callback invoked when a button on a progress dialog is clicked. Returns `true` to keep
/// the dialog open or `false` to close it. This type is public only because it appears in
/// [`DialogMessageRequest`]; callers pass a closure to [`show_progress_with_callback`] rather than
/// constructing this directly.
pub struct ProgressButtonCallback(pub(crate) ProgressButtonCallbackFn);

impl std::fmt::Debug for ProgressButtonCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<callback>")
    }
}

/// A proxy object to control a progress dialog. See `show_progress` for more information.
pub struct ProgressDialogProxy {
    id: usize,
    silent: bool,
    /// When `true`, the dialog is closed when this proxy is dropped. The proxy handed to a button
    /// callback is non-owning (`false`) so it does not close the dialog when it goes out of scope.
    owned: bool,
}

impl ProgressDialogProxy {
    /// Constructs a non-owning proxy for an existing dialog. Dropping it does not close the dialog.
    /// Used by backends to hand a controllable proxy to a button callback.
    pub(crate) fn non_owning(id: usize) -> Self {
        ProgressDialogProxy { id, silent: false, owned: false }
    }

    /// Sets the progress bar to indeterminate mode.
    pub fn set_indeterminate(&self) -> Result<(), XDialogError> {
        if self.silent { return Ok(()); }
        send_request(DialogMessageRequest::SetProgressIndeterminate(self.id))
    }

    /// Sets the progress bar to a specific value between 0.0 and 1.0. Values outside that range
    /// are clamped (e.g. `50.0` becomes `1.0`), matching the native progress controls.
    pub fn set_value(&self, value: f32) -> Result<(), XDialogError> {
        if self.silent { return Ok(()); }
        send_request(DialogMessageRequest::SetProgressValue(self.id, value.clamp(0.0, 1.0)))
    }

    /// Sets the text displayed below the progress bar.
    pub fn set_text<P: AsRef<str>>(&self, text: P) -> Result<(), XDialogError> {
        if self.silent { return Ok(()); }
        send_request(DialogMessageRequest::SetProgressText(self.id, text.as_ref().to_string()))
    }

    /// Closes the progress dialog.
    pub fn close(&self) -> Result<(), XDialogError> {
        if self.silent { return Ok(()); }
        send_request(DialogMessageRequest::CloseWindow(self.id))
    }
}

impl Drop for ProgressDialogProxy {
    fn drop(&mut self) {
        if self.owned {
            let _ = self.close();
        }
    }
}
