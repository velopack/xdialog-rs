use std::{collections::HashMap, sync::mpsc::Receiver, time::Duration};

use dpi::PhysicalSize;
use winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowAttributes, WindowId},
};
use wry::WebViewBuilder;

use crate::{
    sys::{
        mshtml::{self, builder::WebView},
        taskdialog::*,
    },
    ResultSender, XDialogWebviewOptions,
};

use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{SetWindowPos, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER},
};

use crate::{DialogMessageRequest, WebviewDialogProxy, WebviewInvokeHandler, XDialogError, XDialogTheme};

use super::XDialogBackendImpl;

pub struct NativeBackend;

struct NativeApp<'a> {
    pub receiver: Receiver<DialogMessageRequest>,
    pub mshtml_views: HashMap<usize, WebView<'a, UserData>>,
    pub wry_views: HashMap<usize, (Window, wry::WebView)>,
    pub webview2: bool,
}

struct UserData {
    pub id: usize,
    pub cb: Option<WebviewInvokeHandler>,
}

impl<'a> ApplicationHandler for NativeApp<'a> {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn new_events(&mut self, event_loop: &ActiveEventLoop, _cause: StartCause) {
        loop {
            // read all messages until there are no more queued
            let message = self.receiver.try_recv().unwrap_or(DialogMessageRequest::None);

            match message {
                DialogMessageRequest::None => {
                    // sleep for a bit to avoid busy waiting
                    std::thread::sleep(Duration::from_millis(16));
                    return;
                }
                DialogMessageRequest::ShowMessageWindow(id, data) => {
                    task_dialog_show(id, data, false);
                }
                DialogMessageRequest::ExitEventLoop => {
                    task_dialog_close_all();
                    // windows are closed when they are dropped.
                    self.mshtml_views.clear();
                    self.wry_views.clear();
                    event_loop.exit();
                    return;
                }
                DialogMessageRequest::CloseWindow(id) => {
                    // windows are closed when they are dropped.
                    self.mshtml_views.remove(&id);
                    self.wry_views.remove(&id);
                    task_dialog_close(id);
                }
                DialogMessageRequest::ShowProgressWindow(id, data) => {
                    task_dialog_show(id, data, true);
                }
                DialogMessageRequest::SetProgressIndeterminate(id) => {
                    task_dialog_set_progress_indeterminate(id);
                }
                DialogMessageRequest::SetProgressValue(id, value) => {
                    task_dialog_set_progress_value(id, value);
                }
                DialogMessageRequest::SetProgressText(id, text) => {
                    task_dialog_set_progress_text(id, &text);
                }
                DialogMessageRequest::WebviewWindowShow(id, options, result_sender) => {
                    if self.webview2 {
                        self.wry_show(options, id, result_sender, event_loop);
                    } else {
                        self.mshtml_show(options, id, result_sender);
                    }
                }
                DialogMessageRequest::WebviewSetTitle(id, title, result_sender) => {
                    if self.webview2 {
                        self.wry_set_title(id, title, result_sender);
                    } else {
                        self.mshtml_set_title(id, title, result_sender);
                    }
                }
                DialogMessageRequest::WebviewSetHtml(id, html, result_sender) => {
                    if self.webview2 {
                        self.wry_set_html(id, html, result_sender);
                    } else {
                        self.mshtml_set_html(id, html, result_sender);
                    }
                }
                DialogMessageRequest::WebviewSetPosition(id, x, y, result_sender) => {
                    if self.webview2 {
                        self.wry_set_position(id, x, y, result_sender);
                    } else {
                        self.mshtml_set_position(id, x, y, result_sender);
                    }
                }
                DialogMessageRequest::WebviewSetSize(id, w, h, result_sender) => {
                    if self.webview2 {
                        self.wry_set_size(id, w, h, result_sender);
                    } else {
                        self.mshtml_set_size(id, w, h, result_sender);
                    }
                }
                DialogMessageRequest::WebviewSetZoomLevel(id, zoom, result_sender) => {
                    if self.webview2 {
                        self.wry_set_zoom(id, zoom, result_sender);
                    } else {
                        self.mshtml_set_zoom(id, zoom, result_sender);
                    }
                }
                DialogMessageRequest::WebviewSetWindowState(id, xdialog_window_state, result_sender) => {
                    if self.webview2 {
                        self.wry_set_state(id, xdialog_window_state, result_sender);
                    } else {
                        self.mshtml_set_state(id, xdialog_window_state, result_sender);
                    }
                }
                DialogMessageRequest::WebviewEval(id, js, result_sender) => {
                    if self.webview2 {
                        self.wry_eval(id, js, result_sender);
                    } else {
                        self.mshtml_eval(id, js, result_sender);
                    }
                }
            }
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, _event: WindowEvent) {}
}

impl<'a> NativeApp<'a> {
    fn wry_show(&mut self, options: XDialogWebviewOptions, id: usize, mut result_sender: ResultSender, event_loop: &ActiveEventLoop) {
        let result = self.wry_show_impl(options, id, event_loop);
        match result {
            Ok((window, webview)) => {
                self.wry_views.insert(id, (window, webview));
                result_sender.send_result(Ok(()));
            }
            Err(e) => {
                result_sender.send_result(Err(e));
            }
        }
    }

    fn wry_show_impl(
        &mut self,
        options: XDialogWebviewOptions,
        id: usize,
        event_loop: &ActiveEventLoop,
    ) -> Result<(Window, wry::WebView), XDialogError> {
        let mut window_attributes = WindowAttributes::default();
        window_attributes.inner_size = options.size.map(|(w, h)| PhysicalSize::new(w, h).into());
        window_attributes.min_inner_size = options.min_size.map(|(w, h)| PhysicalSize::new(w, h).into());
        window_attributes.resizable = !options.fixed_size;
        window_attributes.title = options.title;
        window_attributes.visible = !options.hidden;
        window_attributes.decorations = !options.borderless;

        let window = event_loop.create_window(window_attributes).map_err(|e| XDialogError::SystemError(format!("{:?}", e)))?;
        let webview = WebViewBuilder::new()
            .with_html(options.html)
            .with_ipc_handler(move |request| {
                if let Some(cb) = options.callback {
                    cb(WebviewDialogProxy { id }, request.body().to_string());
                }
            })
            .build(&window)
            .map_err(|e| XDialogError::SystemError(format!("{:?}", e)))?;

        Ok((window, webview))

        // webview.evaluate_script_with_callback(js, callback)
        // let window = WindowBuilder::new()
        //     .with_title("Wry + Winit Example")
        //     .with_inner_size(LogicalSize::new(800.0, 600.0))
        //     .build(&event_loop)
        //     .unwrap();
    }

    fn wry_set_title(&mut self, id: usize, title: String, mut result_sender: ResultSender) {
        if let Some((window, _)) = self.wry_views.get_mut(&id) {
            window.set_title(&title);
            result_sender.send_result(Ok(()));
        }
    }

    fn wry_set_html(&mut self, id: usize, html: String, mut result_sender: ResultSender) {
        if let Some((_, webview)) = self.wry_views.get_mut(&id) {
            match webview.load_html(&html) {
                Ok(_) => result_sender.send_result(Ok(())),
                Err(e) => result_sender.send_result(Err(XDialogError::SystemError(format!("{:?}", e)))),
            }
        }
    }

    fn wry_set_position(&mut self, id: usize, x: i32, y: i32, mut result_sender: ResultSender) {
        if let Some((window, _)) = self.wry_views.get_mut(&id) {
            window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
            result_sender.send_result(Ok(()));
        }
    }

    fn wry_set_size(&mut self, id: usize, w: i32, h: i32, mut result_sender: ResultSender) {
        if let Some((window, _)) = self.wry_views.get_mut(&id) {
            let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(w, h));
            result_sender.send_result(Ok(()));
        }
    }

    fn wry_set_zoom(&mut self, id: usize, zoom: f64, mut result_sender: ResultSender) {
        if let Some((_, webview)) = self.wry_views.get_mut(&id) {
            match webview.zoom(zoom) {
                Ok(_) => result_sender.send_result(Ok(())),
                Err(e) => result_sender.send_result(Err(XDialogError::SystemError(format!("{:?}", e)))),
            }
        }
    }

    fn wry_set_state(&mut self, id: usize, xdialog_window_state: crate::XDialogWindowState, mut result_sender: ResultSender) {
        if let Some((window, _)) = self.wry_views.get_mut(&id) {
            match xdialog_window_state {
                crate::XDialogWindowState::Normal => {
                    window.set_maximized(false);
                    window.set_minimized(false);
                    window.set_visible(true);
                }
                crate::XDialogWindowState::Maximized => {
                    window.set_maximized(true);
                    window.set_visible(true);
                }
                crate::XDialogWindowState::Minimized => {
                    window.set_minimized(true);
                    window.set_visible(true);
                }
                crate::XDialogWindowState::Hidden => {
                    window.set_visible(false);
                }
                crate::XDialogWindowState::Fullscreen => {
                    window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
                    window.set_visible(true);
                }
            }
            result_sender.send_result(Ok(()));
        }
    }

    fn wry_eval(&mut self, id: usize, js: String, mut result_sender: ResultSender) {
        if let Some((_, webview)) = self.wry_views.get_mut(&id) {
            match webview.evaluate_script(&js) {
                Ok(_) => result_sender.send_result(Ok(())),
                Err(e) => result_sender.send_result(Err(XDialogError::SystemError(format!("{:?}", e)))),
            }
        }
    }
}

impl<'a> NativeApp<'a> {
    fn mshtml_show(&mut self, options: XDialogWebviewOptions, id: usize, mut result_sender: ResultSender) {
        let mut builder = mshtml::builder::builder()
            .content(mshtml::builder::Content::Html(options.html))
            .title(options.title)
            .resizable(!options.fixed_size)
            .user_data(UserData { id, cb: options.callback });
        if let Some(size) = options.size {
            builder = builder.size(size.0, size.1);
        }
        if let Some(min_size) = options.min_size {
            builder = builder.min_size(min_size.0, min_size.1);
        }
        if options.hidden {
            builder = builder.visible(false);
        }
        if options.borderless {
            builder = builder.frameless(true);
        }
        // if options.hide_on_close {
        //     builder = builder.hide_instead_of_close(true);
        // }
        builder = builder.invoke_handler(|webview, arg| {
            println!("Webview invoked: {}", arg);
            let user_data = webview.user_data();
            let user_id = user_data.id;
            if let Some(cb) = user_data.cb {
                cb(WebviewDialogProxy { id: user_id }, arg.to_string());
            }
            Ok(())
        });

        match builder.build() {
            Ok(view) => {
                self.mshtml_views.insert(id, view);
                result_sender.send_result(Ok(()));
            }
            Err(e) => {
                let error_msg = format!("Failed to create mshtml webview: {:?}", e);
                result_sender.send_result(Err(XDialogError::SystemError(error_msg)));
            }
        }
    }

    fn mshtml_set_title(&mut self, id: usize, title: String, mut result_sender: ResultSender) {
        if let Some(webview) = self.mshtml_views.get_mut(&id) {
            match webview.set_title(&title) {
                Ok(_) => result_sender.send_result(Ok(())),
                Err(e) => result_sender.send_result(Err(XDialogError::SystemError(format!("{:?}", e)))),
            }
        }
    }

    fn mshtml_set_html(&mut self, id: usize, html: String, mut result_sender: ResultSender) {
        if let Some(webview) = self.mshtml_views.get_mut(&id) {
            match webview.set_html(&html) {
                Ok(_) => result_sender.send_result(Ok(())),
                Err(e) => result_sender.send_result(Err(XDialogError::SystemError(format!("{:?}", e)))),
            }
        }
    }

    fn mshtml_set_position(&mut self, id: usize, x: i32, y: i32, mut result_sender: ResultSender) {
        if let Some(webview) = self.mshtml_views.get_mut(&id) {
            let handle = webview.window_handle();
            match unsafe { SetWindowPos(HWND(handle), HWND(std::ptr::null_mut()), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER) } {
                Ok(()) => result_sender.send_result(Ok(())),
                Err(e) => result_sender.send_result(Err(XDialogError::SystemError(format!("{:?}", e)))),
            }
        }
    }

    fn mshtml_set_size(&mut self, id: usize, w: i32, h: i32, mut result_sender: ResultSender) {
        if let Some(webview) = self.mshtml_views.get_mut(&id) {
            let handle = webview.window_handle();
            match unsafe { SetWindowPos(HWND(handle), HWND(std::ptr::null_mut()), 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER) } {
                Ok(()) => result_sender.send_result(Ok(())),
                Err(e) => result_sender.send_result(Err(XDialogError::SystemError(format!("{:?}", e)))),
            }
        }
    }

    fn mshtml_set_zoom(&mut self, id: usize, zoom: f64, mut result_sender: ResultSender) {
        if let Some(webview) = self.mshtml_views.get_mut(&id) {
            webview.set_zoom_level(zoom);
            result_sender.send_result(Ok(()));
        }
    }

    fn mshtml_set_state(&mut self, id: usize, xdialog_window_state: crate::XDialogWindowState, mut result_sender: ResultSender) {
        if let Some(webview) = self.mshtml_views.get_mut(&id) {
            match xdialog_window_state {
                crate::XDialogWindowState::Normal => {
                    webview.set_maximized(false);
                    webview.set_minimized(false);
                    webview.set_visible(true);
                }
                crate::XDialogWindowState::Maximized => {
                    webview.set_maximized(true);
                    webview.set_visible(true);
                }
                crate::XDialogWindowState::Minimized => {
                    webview.set_minimized(true);
                    webview.set_visible(true);
                }
                crate::XDialogWindowState::Hidden => {
                    webview.set_visible(false);
                }
                crate::XDialogWindowState::Fullscreen => {
                    webview.set_fullscreen(true);
                    webview.set_visible(true);
                }
            }
            result_sender.send_result(Ok(()));
        }
    }

    fn mshtml_eval(&mut self, id: usize, js: String, mut result_sender: ResultSender) {
        if let Some(webview) = self.mshtml_views.get_mut(&id) {
            match webview.eval(&js) {
                Ok(_) => result_sender.send_result(Ok(())),
                Err(e) => result_sender.send_result(Err(XDialogError::SystemError(format!("{:?}", e)))),
            }
        }
    }
}

impl XDialogBackendImpl for NativeBackend {
    fn run_loop(receiver: Receiver<DialogMessageRequest>, _theme: XDialogTheme) {
        let event_loop = EventLoop::new().unwrap();
        event_loop.set_control_flow(ControlFlow::Poll);
        let mut app = NativeApp {
            receiver,
            mshtml_views: HashMap::new(),
            //wry::webview_version().is_ok()
            webview2: false,
            wry_views: HashMap::new(),
        };
        event_loop.run_app(&mut app).unwrap();
    }
}
