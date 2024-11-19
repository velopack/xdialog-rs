use std::{collections::HashMap, sync::mpsc::Receiver, time::Duration};

use winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::WindowId,
};

use crate::sys::{mshtml, mshtml::builder::WebView, taskdialog::*};

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
                    event_loop.exit();
                    return;
                }
                DialogMessageRequest::CloseWindow(id) => {
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
                    self.mshtml_show(options, id, result_sender);
                }
                DialogMessageRequest::WebviewSetTitle(id, title) => {
                    if let Some(webview) = self.mshtml_views.get_mut(&id) {
                        let _ = webview.set_title(&title);
                    }
                }
                DialogMessageRequest::WebviewSetHtml(id, html) => {
                    if let Some(webview) = self.mshtml_views.get_mut(&id) {
                        let _ = webview.set_html(&html);
                    }
                }
                DialogMessageRequest::WebviewSetPosition(id, x, y) => {
                    if let Some(webview) = self.mshtml_views.get_mut(&id) {
                        let handle = webview.window_handle();
                        let _ = unsafe { SetWindowPos(HWND(handle), HWND(std::ptr::null_mut()), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER) };
                    }
                }
                DialogMessageRequest::WebviewSetSize(id, w, h) => {
                    if let Some(webview) = self.mshtml_views.get_mut(&id) {
                        let handle = webview.window_handle();
                        let _ = unsafe { SetWindowPos(HWND(handle), HWND(std::ptr::null_mut()), 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER) };
                    }
                }
                DialogMessageRequest::WebviewSetZoomLevel(id, zoom) => {
                    if let Some(webview) = self.mshtml_views.get_mut(&id) {
                        webview.set_zoom_level(zoom);
                    }
                }
                DialogMessageRequest::WebviewSetWindowState(id, xdialog_window_state) => {
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
                    }
                }
                DialogMessageRequest::WebviewEval(id, js) => {
                    if let Some(webview) = self.mshtml_views.get_mut(&id) {
                        let _ = webview.eval(&js);
                    }
                }
            }
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, _event: WindowEvent) {}
}

impl<'a> NativeApp<'a> {
    fn mshtml_show(&mut self, options: crate::XDialogWebviewOptions, id: usize, mut result_sender: crate::ResultSender) {
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
        if options.hide_on_close {
            builder = builder.hide_instead_of_close(true);
        }
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
                result_sender.send_result(Err(XDialogError::GenericError(error_msg)));
            }
        }
    }
}

impl XDialogBackendImpl for NativeBackend {
    fn run_loop(receiver: Receiver<DialogMessageRequest>, _theme: XDialogTheme) {
        let event_loop = EventLoop::new().unwrap();
        event_loop.set_control_flow(ControlFlow::Poll);
        let mut app = NativeApp { receiver, mshtml_views: HashMap::new() };
        event_loop.run_app(&mut app).unwrap();
    }
}
