use std::sync::mpsc::Receiver;

use winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::WindowId,
};

use crate::{
    backends::{DialogManager, WebviewManager, XDialogBackendImpl},
    sys, DialogMessageRequest, XDialogTheme,
};

pub struct NativeBackend;

struct NativeApp {
    pub receiver: Receiver<DialogMessageRequest>,
    pub dialogs: Box<dyn DialogManager>,
    pub webviews: Box<dyn WebviewManager>,
}

impl ApplicationHandler for NativeApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn new_events(&mut self, event_loop: &ActiveEventLoop, _cause: StartCause) {
        loop {
            // read all messages until there are no more queued
            let message = self.receiver.try_recv().unwrap_or(DialogMessageRequest::None);

            match message {
                DialogMessageRequest::None => {
                    // sleep for a bit to avoid busy waiting
                    // event_loop.set_control_flow(ControlFlow::WaitUntil(std::time::Instant::now() + Duration::from_millis(16)));
                    // std::thread::sleep(Duration::from_millis(16));
                    return;
                }
                DialogMessageRequest::ShowMessageWindow(id, options, mut result) => {
                    result.send_result(self.dialogs.show(id, options, false));
                }
                DialogMessageRequest::ExitEventLoop => {
                    self.dialogs.close_all();
                    self.webviews.close_all();
                    event_loop.exit();
                    return;
                }
                DialogMessageRequest::CloseWindow(id) => {
                    self.dialogs.close(id);
                    self.webviews.close(id);
                }
                DialogMessageRequest::ShowProgressWindow(id, options, mut result) => {
                    result.send_result(self.dialogs.show(id, options, true));
                }
                DialogMessageRequest::SetProgressIndeterminate(id) => {
                    self.dialogs.set_progress_indeterminate(id);
                }
                DialogMessageRequest::SetProgressValue(id, value) => {
                    self.dialogs.set_progress_value(id, value);
                }
                DialogMessageRequest::SetProgressText(id, text) => {
                    self.dialogs.set_progress_text(id, &text);
                }
                DialogMessageRequest::WebviewWindowShow(id, options, mut result) => {
                    result.send_result(self.webviews.show(id, options, event_loop));
                }
                DialogMessageRequest::WebviewSetTitle(id, title, mut result) => {
                    result.send_result(self.webviews.set_title(id, &title));
                }
                DialogMessageRequest::WebviewSetHtml(id, html, mut result) => {
                    result.send_result(self.webviews.set_html(id, &html));
                }
                DialogMessageRequest::WebviewSetPosition(id, x, y, mut result) => {
                    result.send_result(self.webviews.set_position(id, x, y));
                }
                DialogMessageRequest::WebviewSetSize(id, w, h, mut result) => {
                    result.send_result(self.webviews.set_size(id, w, h));
                }
                DialogMessageRequest::WebviewSetZoomLevel(id, zoom, mut result) => {
                    result.send_result(self.webviews.set_zoom_level(id, zoom));
                }
                DialogMessageRequest::WebviewSetWindowState(id, state, mut result) => {
                    result.send_result(self.webviews.set_window_state(id, state));
                }
                DialogMessageRequest::WebviewEval(id, js, mut result) => {
                    result.send_result(self.webviews.eval_js(id, &js));
                }
            }
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, _event: WindowEvent) {}
}

impl XDialogBackendImpl for NativeBackend {
    fn run_loop(receiver: Receiver<DialogMessageRequest>, _theme: XDialogTheme) {
        let event_loop = EventLoop::new().unwrap();
        event_loop.set_control_flow(ControlFlow::Poll);
        let mut app = NativeApp {
            receiver,
            dialogs: Box::new(sys::taskdialog::TaskDialogManager::new()),
            webviews: if wry::webview_version().is_ok() {
                Box::new(sys::webview2::WryWebview2Manager::new())
            } else {
                Box::new(sys::mshtml::MshtmlWebviewManager::new())
            },
        };
        event_loop.run_app(&mut app).unwrap();
    }
}
