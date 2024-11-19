use crate::*;

pub struct WebviewDialogProxy {
    pub(crate) id: usize,
}

impl WebviewDialogProxy {
    pub fn set_html<S: AsRef<str>>(&self, html: S) -> Result<(), XDialogError> {
        Ok(())
    }

    pub fn set_title<S: AsRef<str>>(&self, title: S) -> Result<(), XDialogError> {
        Ok(())
    }

    // pub fn set_position(&self, x: i32, y: i32) -> Result<(), XDialogError> {
    //     Ok(())
    // }

    pub fn set_size(&self, width: i32, height: i32) -> Result<(), XDialogError> {
        Ok(())
    }

    pub fn set_zoom_level(&self, zoom_level: f64) -> Result<(), XDialogError> {
        Ok(())
    }

    pub fn set_window_state(&self, state: XDialogWindowState) -> Result<(), XDialogError> {
        Ok(())
    }

    pub fn set_resizable(&self, resizable: bool) -> Result<(), XDialogError> {
        Ok(())
    }

    pub fn set_min_size(&self, width: i32, height: i32) -> Result<(), XDialogError> {
        Ok(())
    }

    pub fn eval_js<S: AsRef<str>>(&self, js: S) -> Result<(), XDialogError> {
        Ok(())
    }

    pub fn close() -> Result<(), XDialogError> {
        Ok(())
    }
}

pub fn show_webview(options: XDialogWebviewOptions) -> Result<WebviewDialogProxy, XDialogError> {
    let id = get_next_id();
    let (result_sender, result_receiver) = ResultSender::create();
    send_request(DialogMessageRequest::ShowWebviewWindow(id, options, result_sender))?;
    result_receiver.recv().map_err(|e| XDialogError::NoResult(e))?;
    Ok(WebviewDialogProxy { id })
}
