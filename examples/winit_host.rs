//! An application that runs its own winit event loop and shows xdialog's dialogs inside it.
//!
//! ```sh
//! cargo run --example winit_host --features winit-host
//! ```
//!
//! A worker thread asks a question and shows a progress dialog. Clicking the host window toggles
//! a progress dialog shown from the event-loop thread itself, where blocking calls (`show_message*`)
//! fail with `BlockingCallOnUiThread`. Close the host window to quit.
//!
//! The handler has no xdialog code: `into_host_app` wraps it and handles the dialogs' windows.

use std::time::Duration;

use xdialog::host::winit::application::ApplicationHandler;
use xdialog::host::winit::event::{ElementState, WindowEvent};
use xdialog::host::winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use xdialog::host::winit::window::{Window, WindowId};
use xdialog::*;

enum UserEvent {
    /// xdialog's waker: nothing to do here.
    XDialog,
    /// The worker thread finished.
    WorkerDone,
}

struct App {
    window: Option<Window>,
    progress: Option<ProgressDialogProxy>,
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_none() {
            let attrs = Window::default_attributes().with_title("winit host (click me)");
            self.window = Some(el.create_window(attrs).unwrap());
        }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::XDialog => {}
            UserEvent::WorkerDone => println!("worker finished; close the host window to quit"),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.window.as_ref().is_none_or(|w| w.id() != id) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::MouseInput { state: ElementState::Pressed, .. } => self.toggle_progress(),
            _ => {}
        }
    }
}

impl App {
    /// Dialogs shown from the event-loop thread.
    fn toggle_progress(&mut self) {
        if self.progress.take().is_some() {
            return; // dropping the proxy closes the dialog
        }
        // Blocking here would deadlock the loop that has to show the dialog.
        let err = show_message_info_ok("winit host", "Blocking", "Never shown").unwrap_err();
        println!("show_message on the event-loop thread: {err}");
        // Non-blocking calls work; the window appears in the next `about_to_wait`.
        let progress = show_progress("winit host", "Event-loop thread", "Click the host window to close.", XDialogIcon::Information).unwrap();
        progress.set_indeterminate().unwrap();
        self.progress = Some(progress);
    }
}

fn worker(proxy: EventLoopProxy<UserEvent>) {
    if show_message_yes_no("winit host", "Worker thread", "Show a progress dialog?", XDialogIcon::Information).unwrap() {
        let progress = show_progress("winit host", "Worker thread", "Working...", XDialogIcon::None).unwrap();
        for i in 0..=50 {
            progress.set_value(i as f32 / 50.0).unwrap();
            std::thread::sleep(Duration::from_millis(60));
        }
        progress.close().unwrap();
    }
    let _ = proxy.send_event(UserEvent::WorkerDone);
}

fn main() {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build().unwrap();
    let waker = event_loop.create_proxy();
    let mut app = XDialogBuilder::new().into_host_app(App { window: None, progress: None }, move || {
                                           let _ = waker.send_event(UserEvent::XDialog);
                                       })
                                       .unwrap();
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || worker(proxy));
    event_loop.run_app(&mut app).unwrap();
}
