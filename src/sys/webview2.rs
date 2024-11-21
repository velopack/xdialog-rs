use std::{collections::HashMap, thread};

use dpi::PhysicalSize;
use winit::window::{Window, WindowAttributes};
use wry::WebViewBuilder;

use crate::{backends::WebviewManager, WebviewDialogProxy, XDialogError, XDialogWebviewOptions};

pub struct WryWebview2Manager {
    wry_views: HashMap<usize, (Window, wry::WebView)>,
}

impl WryWebview2Manager {
    pub fn new() -> Self {
        WryWebview2Manager { wry_views: HashMap::new() }
    }
}

impl WebviewManager for WryWebview2Manager {
    fn show(
        &mut self,
        id: usize,
        options: XDialogWebviewOptions,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) -> Result<(), XDialogError> {
        let title = options.title;
        let html = options.html;
        let size = options.size;
        let position = options.position;
        let min_size = options.min_size;
        let resizable = options.resizable;
        let borderless = options.borderless;
        let state = options.state;
        let callback = options.callback;

        let mut window_attributes = WindowAttributes::default();
        window_attributes.inner_size = size.map(|(w, h)| PhysicalSize::new(w, h).into());
        window_attributes.min_inner_size = min_size.map(|(w, h)| PhysicalSize::new(w, h).into());
        window_attributes.resizable = resizable;
        window_attributes.title = title;
        window_attributes.decorations = !borderless;
        window_attributes.visible = false;
        window_attributes.position = position.map(|(x, y)| winit::dpi::PhysicalPosition::new(x, y).into());

        let window = event_loop.create_window(window_attributes).map_err(|e| XDialogError::SystemError(format!("{:?}", e)))?;
        let webview = WebViewBuilder::new()
            .with_html(html)
            .with_ipc_handler(move |request| {
                if let Some(cb) = callback {
                    let arg = request.body().to_string();
                    thread::spawn(move || {
                        cb(WebviewDialogProxy { id }, arg);
                    });
                }
            })
            .build(&window)
            .map_err(|e| XDialogError::SystemError(format!("{:?}", e)))?;

        match state {
            crate::XDialogWindowState::Hidden => {}
            crate::XDialogWindowState::Normal => {
                window.set_visible(true);
            }
            crate::XDialogWindowState::Minimized => {
                window.set_minimized(true);
                window.set_visible(true);
            }
            crate::XDialogWindowState::Maximized => {
                window.set_maximized(true);
                window.set_visible(true);
            }
            crate::XDialogWindowState::FullscreenBorderless => {
                window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
                window.set_visible(true);
            }
        }

        self.wry_views.insert(id, (window, webview));
        Ok(())
    }

    fn set_title(&mut self, id: usize, title: &str) -> Result<(), XDialogError> {
        if let Some((window, _)) = self.wry_views.get_mut(&id) {
            window.set_title(title);
        }
        Ok(())
    }

    fn set_html(&mut self, id: usize, html: &str) -> Result<(), XDialogError> {
        if let Some((_, webview)) = self.wry_views.get_mut(&id) {
            webview.load_html(html).map_err(|e| XDialogError::SystemError(format!("{:?}", e)))?;
        }
        Ok(())
    }

    fn set_position(&mut self, id: usize, x: i32, y: i32) -> Result<(), XDialogError> {
        if let Some((window, _)) = self.wry_views.get_mut(&id) {
            window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
        }
        Ok(())
    }

    fn set_size(&mut self, id: usize, width: i32, height: i32) -> Result<(), XDialogError> {
        if let Some((window, _)) = self.wry_views.get_mut(&id) {
            let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(width, height));
        }
        Ok(())
    }

    fn set_zoom_level(&mut self, id: usize, zoom_level: f64) -> Result<(), XDialogError> {
        if let Some((_, webview)) = self.wry_views.get_mut(&id) {
            webview.zoom(zoom_level).map_err(|e| XDialogError::SystemError(format!("{:?}", e)))?;
        }
        Ok(())
    }

    fn set_window_state(&mut self, id: usize, state: crate::XDialogWindowState) -> Result<(), XDialogError> {
        if let Some((window, _)) = self.wry_views.get_mut(&id) {
            match state {
                crate::XDialogWindowState::Normal => {
                    window.set_fullscreen(None);
                    window.set_maximized(false);
                    window.set_minimized(false);
                    window.set_visible(true);
                }
                crate::XDialogWindowState::Maximized => {
                    window.set_fullscreen(None);
                    window.set_maximized(true);
                    window.set_visible(true);
                }
                crate::XDialogWindowState::Minimized => {
                    window.set_fullscreen(None);
                    window.set_minimized(true);
                    window.set_visible(true);
                }
                crate::XDialogWindowState::Hidden => {
                    window.set_visible(false);
                }
                crate::XDialogWindowState::FullscreenBorderless => {
                    window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
                    window.set_visible(true);
                }
            }
        }
        Ok(())
    }

    fn eval_js(&mut self, id: usize, js: &str) -> Result<(), XDialogError> {
        if let Some((_, webview)) = self.wry_views.get_mut(&id) {
            webview.evaluate_script(js).map_err(|e| XDialogError::SystemError(format!("{:?}", e)))?;
        }
        Ok(())
    }

    fn close(&mut self, id: usize) {
        self.wry_views.remove(&id);
    }

    fn close_all(&mut self) {
        self.wry_views.clear();
    }
}
