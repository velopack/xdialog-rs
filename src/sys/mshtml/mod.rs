use std::{collections::HashMap, thread};

use builder::WebView;
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{SetWindowPos, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER},
};
use winit::event_loop::ActiveEventLoop;

use crate::{backends::WebviewManager, WebviewDialogProxy, WebviewInvokeHandler, XDialogError, XDialogWebviewOptions, XDialogWindowState};

mod builder;
mod color;
mod error;
mod escape;
mod native;

struct UserData {
    pub id: usize,
    pub cb: Option<WebviewInvokeHandler>,
}

pub struct MshtmlWebviewManager<'a> {
    webviews: HashMap<usize, WebView<'a, UserData>>,
}

impl MshtmlWebviewManager<'_> {
    pub fn new() -> Self {
        MshtmlWebviewManager { webviews: HashMap::new() }
    }
}

impl WebviewManager for MshtmlWebviewManager<'_> {
    fn show(&mut self, id: usize, options: XDialogWebviewOptions, _event_loop: &ActiveEventLoop) -> Result<(), XDialogError> {
        let title = options.title;
        let html = options.html;
        let size = options.size;
        let position = options.position;
        let min_size = options.min_size;
        let resizable = options.resizable;
        let borderless = options.borderless;
        let state = options.state;
        let callback = options.callback;

        let mut builder = builder::builder()
            .content(builder::Content::Html(html))
            .title(title)
            .resizable(resizable)
            .visible(false)
            .frameless(borderless)
            .user_data(UserData { id, cb: callback });

        if let Some(size) = size {
            builder = builder.size(size.0, size.1);
        }

        if let Some(min_size) = min_size {
            builder = builder.min_size(min_size.0, min_size.1);
        }

        builder = builder.invoke_handler(|webview, arg| {
            println!("Webview invoked: {}", arg);
            let user_data = webview.user_data();
            let user_id = user_data.id;
            if let Some(cb) = user_data.cb {
                let arg = arg.to_string();
                thread::spawn(move || {
                    cb(WebviewDialogProxy { id: user_id }, arg);
                });
            }
            Ok(())
        });

        match builder.build() {
            Ok(view) => {
                self.webviews.insert(id, view);

                if let Some((x, y)) = position {
                    self.set_position(id, x, y)?;
                }

                self.set_window_state(id, state)?;
                Ok(())
            }
            Err(e) => Err(XDialogError::SystemError(format!("Failed to create mshtml webview: {:?}", e))),
        }
    }

    fn set_title(&mut self, id: usize, title: &str) -> Result<(), XDialogError> {
        if let Some(webview) = self.webviews.get_mut(&id) {
            webview.set_title(title).map_err(|e| XDialogError::SystemError(format!("{:?}", e)))?;
        }
        Ok(())
    }

    fn set_html(&mut self, id: usize, html: &str) -> Result<(), XDialogError> {
        if let Some(webview) = self.webviews.get_mut(&id) {
            webview.set_html(html).map_err(|e| XDialogError::SystemError(format!("{:?}", e)))?;
        }
        Ok(())
    }

    fn set_position(&mut self, id: usize, x: i32, y: i32) -> Result<(), XDialogError> {
        if let Some(webview) = self.webviews.get_mut(&id) {
            let handle = webview.window_handle();
            unsafe {
                SetWindowPos(HWND(handle), HWND(std::ptr::null_mut()), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER)
                    .map_err(|e| XDialogError::SystemError(format!("{:?}", e)))?;
            }
        }
        Ok(())
    }

    fn set_size(&mut self, id: usize, width: i32, height: i32) -> Result<(), XDialogError> {
        if let Some(webview) = self.webviews.get_mut(&id) {
            let handle = webview.window_handle();
            unsafe {
                SetWindowPos(HWND(handle), HWND(std::ptr::null_mut()), 0, 0, width, height, SWP_NOMOVE | SWP_NOZORDER)
                    .map_err(|e| XDialogError::SystemError(format!("{:?}", e)))?;
            }
        }
        Ok(())
    }

    fn set_zoom_level(&mut self, id: usize, zoom_level: f64) -> Result<(), XDialogError> {
        if let Some(webview) = self.webviews.get_mut(&id) {
            webview.set_zoom_level(zoom_level);
        }
        Ok(())
    }

    fn set_window_state(&mut self, id: usize, state: XDialogWindowState) -> Result<(), XDialogError> {
        if let Some(webview) = self.webviews.get_mut(&id) {
            match state {
                XDialogWindowState::Normal => {
                    webview.set_fullscreen(false);
                    webview.set_maximized(false);
                    webview.set_minimized(false);
                    webview.set_visible(true);
                }
                XDialogWindowState::Maximized => {
                    webview.set_maximized(true);
                    webview.set_visible(true);
                }
                XDialogWindowState::Minimized => {
                    webview.set_minimized(true);
                    webview.set_visible(true);
                }
                XDialogWindowState::Hidden => {
                    webview.set_visible(false);
                }
                XDialogWindowState::FullscreenBorderless => {
                    webview.set_fullscreen(true);
                    webview.set_visible(true);
                }
            }
        }
        Ok(())
    }

    fn eval_js(&mut self, id: usize, js: &str) -> Result<(), XDialogError> {
        if let Some(webview) = self.webviews.get_mut(&id) {
            webview.eval(js).map_err(|e| XDialogError::SystemError(format!("{:?}", e)))?;
        }
        Ok(())
    }

    fn close(&mut self, id: usize) {
        self.webviews.remove(&id);
    }

    fn close_all(&mut self) {
        self.webviews.clear();
    }
}
