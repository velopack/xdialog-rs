use crate::*;

/// A proxy object to control a webview dialog. See `show_webview` for more information.
pub struct WebviewDialogProxy {
    pub(crate) id: usize,
}

impl WebviewDialogProxy {
    /// Set the HTML content of the webview.
    pub fn set_html<S: AsRef<str>>(&self, html: S) -> Result<(), XDialogError> {
        let (result_sender, result_receiver) = ResultSender::create();
        send_request(DialogMessageRequest::WebviewSetHtml(self.id, html.as_ref().to_string(), result_sender))?;
        let _ = result_receiver.recv().map_err(|e| XDialogError::NoResult(e))?;
        Ok(())
    }

    /// Set the title of the webview window.
    pub fn set_title<S: AsRef<str>>(&self, title: S) -> Result<(), XDialogError> {
        let (result_sender, result_receiver) = ResultSender::create();
        send_request(DialogMessageRequest::WebviewSetTitle(self.id, title.as_ref().to_string(), result_sender))?;
        let _ = result_receiver.recv().map_err(|e| XDialogError::NoResult(e))?;
        Ok(())
    }

    /// Set the position of the webview window.
    pub fn set_position(&self, x: i32, y: i32) -> Result<(), XDialogError> {
        let (result_sender, result_receiver) = ResultSender::create();
        send_request(DialogMessageRequest::WebviewSetPosition(self.id, x, y, result_sender))?;
        let _ = result_receiver.recv().map_err(|e| XDialogError::NoResult(e))?;
        Ok(())
    }

    /// Set the size of the webview window.
    pub fn set_size(&self, width: i32, height: i32) -> Result<(), XDialogError> {
        let (result_sender, result_receiver) = ResultSender::create();
        send_request(DialogMessageRequest::WebviewSetSize(self.id, width, height, result_sender))?;
        let _ = result_receiver.recv().map_err(|e| XDialogError::NoResult(e))?;
        Ok(())
    }

    /// Set the zoom level of the webview window.
    pub fn set_zoom_level(&self, zoom_level: f64) -> Result<(), XDialogError> {
        let (result_sender, result_receiver) = ResultSender::create();
        send_request(DialogMessageRequest::WebviewSetZoomLevel(self.id, zoom_level, result_sender))?;
        let _ = result_receiver.recv().map_err(|e| XDialogError::NoResult(e))?;
        Ok(())
    }

    /// Set the window state of the webview window.
    pub fn set_window_state(&self, state: XDialogWindowState) -> Result<(), XDialogError> {
        let (result_sender, result_receiver) = ResultSender::create();
        send_request(DialogMessageRequest::WebviewSetWindowState(self.id, state, result_sender))?;
        let _ = result_receiver.recv().map_err(|e| XDialogError::NoResult(e))?;
        Ok(())
    }

    /// Evaluate JavaScript in the webview.
    pub fn eval_js<S: AsRef<str>>(&self, js: S) -> Result<(), XDialogError> {
        let (result_sender, result_receiver) = ResultSender::create();
        send_request(DialogMessageRequest::WebviewEval(self.id, js.as_ref().to_string(), result_sender))?;
        let _ = result_receiver.recv().map_err(|e| XDialogError::NoResult(e))?;
        Ok(())
    }

    /// Close the webview window.
    pub fn close(&self) -> Result<(), XDialogError> {
        send_request(DialogMessageRequest::CloseWindow(self.id))?;
        Ok(())
    }
}

/// Shows a webview dialog with the specified options and returns a proxy object to control it.
pub fn show_webview(options: XDialogWebviewOptions) -> Result<WebviewDialogProxy, XDialogError> {
    let id = get_next_id();
    let (result_sender, result_receiver) = ResultSender::create();
    send_request(DialogMessageRequest::WebviewWindowShow(id, options, result_sender))?;
    let _ = result_receiver.recv().map_err(|e| XDialogError::NoResult(e))?;
    Ok(WebviewDialogProxy { id })
}
