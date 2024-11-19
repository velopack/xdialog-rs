use std::sync::mpsc::channel;

use crate::errors::*;
use crate::model::*;
use crate::state::*;

pub struct WebviewDialogProxy {
    id: usize,
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

    pub fn close() -> Result<(), XDialogError> {
        Ok(())
    }
}

pub fn show_webview(options: XDialogWebviewOptions) -> Result<WebviewDialogProxy, XDialogError> {
    let (sender, receiver) = channel();
    let id = get_next_id();
    send_request(DialogMessageRequest::ShowWebviewWindow(id, options, sender.into()))?;
    Ok(WebviewDialogProxy { id })
}
