use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use crate::channel::is_ui_thread;
use crate::model::DialogReply;
use crate::oneshot::{self, Wait};
use crate::*;

/// Shows a message box with an information icon and an OK button and blocks until the user closes it.
pub fn show_message_info_ok<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    window_title: P1,
    main_instruction: P2,
    message: P3,
) -> Result<(), XDialogError> {
    show_message_internal(XDialogIcon::Information, window_title, main_instruction, message, vec!["OK".to_string()])?;
    Ok(())
}

/// Shows a message box with a warning icon and an OK button and blocks until the user closes it.
pub fn show_message_warn_ok<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    window_title: P1,
    main_instruction: P2,
    message: P3,
) -> Result<(), XDialogError> {
    show_message_internal(XDialogIcon::Warning, window_title, main_instruction, message, vec!["OK".to_string()])?;
    Ok(())
}

/// Shows a message box with an error icon and an OK button and blocks until the user closes it.
pub fn show_message_error_ok<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    window_title: P1,
    main_instruction: P2,
    message: P3,
) -> Result<(), XDialogError> {
    show_message_internal(XDialogIcon::Error, window_title, main_instruction, message, vec!["OK".to_string()])?;
    Ok(())
}

/// Shows a message box with OK/Cancel buttons and blocks until the user closes it.
/// Returns `true` if the OK button was pressed, `false` if the Cancel button was pressed or the dialog was closed.
pub fn show_message_ok_cancel<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    window_title: P1,
    main_instruction: P2,
    message: P3,
    icon: XDialogIcon,
) -> Result<bool, XDialogError> {
    let result = show_message_internal(icon, window_title, main_instruction, message, vec!["Cancel".to_string(), "OK".to_string()])?;
    Ok(result == XDialogResult::ButtonPressed(1))
}

/// Shows a message box with Yes/No buttons and blocks until the user closes it.
/// Returns `true` if the Yes button was pressed, `false` if the No button was pressed or the dialog was closed.
pub fn show_message_yes_no<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    window_title: P1,
    main_instruction: P2,
    message: P3,
    icon: XDialogIcon,
) -> Result<bool, XDialogError> {
    let result = show_message_internal(icon, window_title, main_instruction, message, vec!["No".to_string(), "Yes".to_string()])?;
    Ok(result == XDialogResult::ButtonPressed(1))
}

/// Shows a message box with Retry/Cancel buttons and blocks until the user closes it.
/// Returns `true` if the Retry button was pressed, `false` if the Cancel button was pressed or the dialog was closed.
pub fn show_message_retry_cancel<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    window_title: P1,
    main_instruction: P2,
    message: P3,
    icon: XDialogIcon,
) -> Result<bool, XDialogError> {
    let result = show_message_internal(icon, window_title, main_instruction, message, vec!["Cancel".to_string(), "Retry".to_string()])?;
    Ok(result == XDialogResult::ButtonPressed(1))
}

fn show_message_internal<P1: AsRef<str>, P2: AsRef<str>, P3: AsRef<str>>(
    icon: XDialogIcon,
    window_title: P1,
    main_instruction: P2,
    message: P3,
    buttons: Vec<String>,
) -> Result<XDialogResult, XDialogError> {
    let data = XDialogOptions {
        title: window_title.as_ref().to_string(),
        main_instruction: main_instruction.as_ref().to_string(),
        message: message.as_ref().to_string(),
        icon,
        buttons,
    };
    // Checked before showing: the dialog would only flash.
    if is_ui_thread() && !get_silent() {
        warn!("xdialog: show_message_* would block the xdialog UI thread and deadlock; call it from another thread or use show_message");
        return Err(XDialogError::BlockingCallOnUiThread);
    }
    show_message(data).wait()
}

/// Shows a message box with the specified options and returns at once, without waiting for the
/// user. Use the returned [`MessageDialogProxy`] to wait for the result, check for it, `.await` it
/// or close the dialog; dropping the proxy closes the dialog.
///
/// Unlike the `show_message_*` shortcuts, which block, this works on xdialog's UI thread too (see
/// [Threads](crate#threads)), e.g. on the event-loop thread of an app using the `winit-host`
/// feature. Errors (no backend, the dialog couldn't be created) are the proxy's result.
///
/// ### Example
/// ```rust,no_run
/// use xdialog::*;
///
/// # fn run() -> Result<(), XDialogError> {
/// let options = XDialogOptions {
///     title: "Save changes?".to_string(),
///     main_instruction: "Save changes before closing?".to_string(),
///     message: "Your changes will be lost if you don't save them.".to_string(),
///     icon: XDialogIcon::Warning,
///     buttons: vec!["Don't save".to_string(), "Save".to_string()],
/// };
/// let dialog = show_message(options);
/// // ... later, e.g. once per event-loop iteration:
/// if let Some(result) = dialog.try_result() {
///     let save = result? == XDialogResult::ButtonPressed(1);
/// }
/// // or block (not on xdialog's UI thread): dialog.wait()?
/// # Ok(())
/// # }
/// ```
pub fn show_message(options: XDialogOptions) -> MessageDialogProxy {
    let (tx, result) = oneshot::channel();
    let proxy = MessageDialogProxy { id: get_next_id(), result };
    if get_silent() {
        proxy.result.set(Ok(XDialogResult::SilentMode));
    } else if let Err(e) = send_request(DialogMessageRequest::ShowMessageWindow(proxy.id, options, DialogReply::Message(tx))) {
        proxy.result.set(Err(e));
    }
    proxy
}

/// A message box shown by [`show_message`]: wait for its result, check for it, `.await` it, or
/// close the dialog.
///
/// The result is the button pressed, or the error that kept the dialog from showing. It is kept:
/// once there, every call returns it again. Dropping the proxy closes the dialog if it is still
/// open.
#[must_use = "dropping the proxy closes the dialog"]
pub struct MessageDialogProxy {
    id: usize,
    /// Set once by the backend (or by a timeout).
    result: oneshot::Receiver<Result<XDialogResult, XDialogError>>,
}

impl MessageDialogProxy {
    fn get(&self, wait: &mut Wait<'_, '_>) -> Option<Result<XDialogResult, XDialogError>> {
        self.result.recv_with(wait).map(|r| r.unwrap_or_else(|e| Err(XDialogError::NoResult(e))))
    }

    /// The result if the dialog has closed (or failed to show), without blocking; `None` while it
    /// is open.
    pub fn try_result(&self) -> Option<Result<XDialogResult, XDialogError>> {
        self.get(&mut Wait::Now)
    }

    /// Blocks until the user closes the dialog. Returns [`XDialogError::BlockingCallOnUiThread`]
    /// on xdialog's UI thread while the dialog is open (see [Threads](crate#threads)).
    pub fn wait(&self) -> Result<XDialogResult, XDialogError> {
        self.wait_until(None)
    }

    /// Like [`wait`](Self::wait), but closes the dialog and returns
    /// [`XDialogResult::TimeoutElapsed`] if the user hasn't closed it within `timeout`.
    pub fn wait_timeout(&self, timeout: Duration) -> Result<XDialogResult, XDialogError> {
        self.wait_until(Some(Instant::now() + timeout))
    }

    fn wait_until(&self, deadline: Option<Instant>) -> Result<XDialogResult, XDialogError> {
        if let Some(result) = self.try_result() {
            return result;
        }
        if is_ui_thread() {
            warn!("xdialog: waiting for a message box would block the xdialog UI thread and deadlock; use try_result or await it");
            return Err(XDialogError::BlockingCallOnUiThread);
        }
        if let Some(result) = self.get(&mut Wait::Until(deadline)) {
            return result;
        }
        // Timed out. The first result wins: one that arrives right now is kept.
        self.result.set(Ok(XDialogResult::TimeoutElapsed));
        let _ = send_request(DialogMessageRequest::CloseWindow(self.id));
        self.try_result().unwrap_or(Ok(XDialogResult::TimeoutElapsed))
    }

    /// Closes the dialog; its result becomes [`XDialogResult::WindowClosed`]. Does nothing if it
    /// has closed.
    pub fn close(&self) -> Result<(), XDialogError> {
        if self.try_result().is_some() {
            return Ok(());
        }
        send_request(DialogMessageRequest::CloseWindow(self.id))
    }
}

impl Future for MessageDialogProxy {
    type Output = Result<XDialogResult, XDialogError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.get(&mut Wait::Poll(cx)) {
            Some(result) => Poll::Ready(result),
            None => Poll::Pending,
        }
    }
}

impl Drop for MessageDialogProxy {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

impl std::fmt::Debug for MessageDialogProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageDialogProxy").field("id", &self.id).finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A proxy fed by the returned reply; no backend: dropping it unanswered only fails to send
    /// the close.
    fn proxy() -> (MessageDialogProxy, DialogReply) {
        let (tx, result) = oneshot::channel();
        (MessageDialogProxy { id: 0, result }, DialogReply::Message(tx))
    }

    #[test]
    fn proxy_result_is_kept() {
        let (p, reply) = proxy();
        assert!(p.try_result().is_none());
        let tx = reply.opened();
        assert!(p.try_result().is_none(), "open, no result yet");
        tx.send(XDialogResult::ButtonPressed(2));
        assert!(matches!(p.try_result(), Some(Ok(XDialogResult::ButtonPressed(2)))));
        assert!(matches!(p.wait(), Ok(XDialogResult::ButtonPressed(2))));
    }

    #[test]
    fn proxy_failure_is_the_result() {
        let (p, reply) = proxy();
        reply.failed(XDialogError::SystemError("no window".into()));
        assert!(matches!(p.wait(), Err(XDialogError::SystemError(e)) if e == "no window"));
        assert!(matches!(p.try_result(), Some(Err(XDialogError::SystemError(_)))));

        let (p, reply) = proxy();
        drop(reply);
        assert!(matches!(p.try_result(), Some(Err(XDialogError::NoResult(_)))), "a dropped request doesn't hang");
    }

    #[test]
    fn proxy_wait_blocks_off_the_ui_thread_only() {
        let (p, reply) = proxy();
        let mark = crate::channel::UiThreadMark::set();
        let r = p.wait();
        drop(mark);
        assert!(matches!(r, Err(XDialogError::BlockingCallOnUiThread)), "{r:?}");

        let answer = std::thread::spawn(move || {
            let tx = reply.opened();
            std::thread::sleep(Duration::from_millis(20));
            tx.send(XDialogResult::WindowClosed);
        });
        assert!(matches!(p.wait(), Ok(XDialogResult::WindowClosed)));
        answer.join().unwrap();
    }

    #[test]
    fn proxy_timeout_is_kept() {
        let (p, reply) = proxy();
        let tx = reply.opened();
        assert!(matches!(p.wait_timeout(Duration::from_millis(10)), Ok(XDialogResult::TimeoutElapsed)));
        tx.send(XDialogResult::WindowClosed); // the backend's close, afterwards
        assert!(matches!(p.try_result(), Some(Ok(XDialogResult::TimeoutElapsed))));
    }

    #[test]
    fn show_message_on_ui_thread_fails_fast() {
        let mark = crate::channel::UiThreadMark::set();
        let r = show_message_info_ok("t", "m", "b");
        drop(mark);
        assert!(matches!(r, Err(XDialogError::BlockingCallOnUiThread)), "{r:?}");
    }
}
