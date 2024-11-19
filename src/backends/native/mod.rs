use std::{
    collections::HashMap,
    sync::mpsc::Receiver,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use builder::WebView;
use windows::Win32::Foundation::HWND;
use winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::WindowId,
};

#[cfg(windows)]
use crate::sys::mshtml::*;
#[cfg(windows)]
use crate::sys::taskdialog::*;

use crate::{sys::mshtml, DialogMessageRequest, XDialogTheme};

use super::XDialogBackendImpl;

pub struct NativeBackend;

pub struct NativeApp<'a> {
    pub receiver: Receiver<DialogMessageRequest>,
    pub theme: XDialogTheme,
    pub webviews: HashMap<usize, WebView<'a, ()>>,
}

impl<'a> ApplicationHandler for NativeApp<'a> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // event_loop
        //     .create_window(Window::default_attributes())
        //     .unwrap();
        // println!("Resumed");
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, _cause: StartCause) {
        loop {
            // read all messages until there are no more queued
            let message = self.receiver.try_recv().unwrap_or(DialogMessageRequest::None);
            if message == DialogMessageRequest::None {
                std::thread::sleep(Duration::from_millis(16));
                break;
            }

            match message {
                DialogMessageRequest::None => {}
                DialogMessageRequest::ShowMessageWindow(id, data) => {
                    #[cfg(windows)]
                    task_dialog_show(id, data, false);
                }
                DialogMessageRequest::ExitEventLoop => {
                    #[cfg(windows)]
                    task_dialog_close_all();
                    event_loop.exit();
                    return;
                }
                DialogMessageRequest::CloseWindow(id) => {
                    #[cfg(windows)]
                    task_dialog_close(id);
                }
                DialogMessageRequest::ShowProgressWindow(id, data) => {
                    #[cfg(windows)]
                    task_dialog_show(id, data, true);
                }
                DialogMessageRequest::SetProgressIndeterminate(id) => {
                    #[cfg(windows)]
                    task_dialog_set_progress_indeterminate(id);
                }
                DialogMessageRequest::SetProgressValue(id, value) => {
                    #[cfg(windows)]
                    task_dialog_set_progress_value(id, value);
                }
                DialogMessageRequest::SetProgressText(id, text) => {
                    #[cfg(windows)]
                    task_dialog_set_progress_text(id, &text);
                }
                DialogMessageRequest::ShowWebviewWindow(id, options, sender) => {
                    let mut builder = mshtml::builder::builder()
                        .content(builder::Content::Html(options.html))
                        .title(options.title)
                        .resizable(options.resizable)
                        .user_data(());
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
                        // use Cmd::*;
            
                        // let tasks_len = {
                        //     let tasks = webview.user_data_mut();
            
                        //     match serde_json::from_str(arg).unwrap() {
                        //         Init => (),
                        //         Log { text } => println!("{}", text),
                        //         AddTask { name } => tasks.push(Task { name, done: false }),
                        //         MarkTask { index, done } => tasks[index].done = done,
                        //         ClearDoneTasks => tasks.retain(|t| !t.done),
                        //     }
            
                        //     tasks.len()
                        // };
            
                        // webview.set_title(&format!("Rust Todo App ({} Tasks)", tasks_len))?;
                        // render(webview)
                        Ok(())
                    });

                    let view = builder.build().unwrap();
                    self.webviews.insert(id, view);
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // println!("Window event: {:?}", event);
        // // let id: u64 = id.into();
        // // let id = id as usize;
        // match event {
        //     WindowEvent::Resized(size) => {
        //         // iterate each window and resize it
        //         for (_, (_, _, _, xaml_island_hwnd)) in self.windows.iter_mut() {
        //             unsafe {
        //                 SetWindowPos(xaml_island_hwnd.clone(), HWND(0), 0, 0, size.width as _, size.height as _, SWP_SHOWWINDOW);
        //             }
        //         }
        //     }
        //     WindowEvent::CloseRequested => {
        //         // self.windows.remove(&id);

        //         // println!("The close button was pressed; stopping");
        //         // event_loop.exit();
        //     }
        //     WindowEvent::RedrawRequested => {
        //         // if let Some(wnd) = self.windows. {
        //         //     wnd.request_redraw();
        //         // }
        //         // Redraw the application.
        //         //
        //         // It's preferable for applications that do not render continuously to render in
        //         // this event rather than in AboutToWait, since rendering in here allows
        //         // the program to gracefully handle redraws requested by the OS.

        //         // Draw.

        //         // Queue a RedrawRequested event.
        //         //
        //         // You only need to call this if you've determined that you need to redraw in
        //         // applications which do not always need to. Applications that redraw continuously
        //         // can render here instead.
        //         // self.window.as_ref().unwrap().request_redraw();
        //     }
        //     _ => (),
        // }
    }
}

impl XDialogBackendImpl for NativeBackend {
    fn run_loop(receiver: Receiver<DialogMessageRequest>, theme: XDialogTheme) {
        let event_loop = EventLoop::new().unwrap();
        event_loop.set_control_flow(ControlFlow::Poll);
        let mut app = NativeApp { receiver, theme, webviews: HashMap::new() };
        event_loop.run_app(&mut app).unwrap();
    }
}
