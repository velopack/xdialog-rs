//! An application that runs its own winit event loop and shows xdialog's dialogs inside it.
//!
//! ```sh
//! cargo run --example winit_host --features winit-host
//! ```
//!
//! A worker thread asks a question and shows a progress dialog, blocking as a console app would.
//! Clicking the host window asks the same question from the event-loop thread itself, where the
//! blocking `show_message_*` shortcuts fail with `BlockingCallOnUiThread`: `show_message` returns
//! at once and the app checks for the answer in `about_to_wait`. Close the host window to quit.
//!
//! `into_host_app` wraps the handler and handles the dialogs' windows.

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
    /// The question asked from the event-loop thread, until answered.
    question: Option<MessageDialogProxy>,
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
            WindowEvent::MouseInput { state: ElementState::Pressed, .. } => self.clicked(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        // xdialog wakes the loop when the answer arrives.
        let Some(answer) = self.question.as_ref().and_then(|q| q.try_result()) else { return };
        self.question = None;
        if let Ok(XDialogResult::ButtonPressed(1)) = answer {
            let progress =
                show_progress("winit host", "Event-loop thread", "Click the host window to close.", XDialogIcon::Information).unwrap();
            progress.set_indeterminate().unwrap();
            self.progress = Some(progress);
        }
    }
}

impl App {
    /// Dialogs shown from the event-loop thread.
    fn clicked(&mut self) {
        if self.progress.take().is_some() || self.question.is_some() {
            return; // dropping the progress proxy closes the dialog
        }
        // Blocking here would deadlock the loop that has to show the dialog.
        let err = show_message_yes_no("winit host", "Blocking", "Never shown", XDialogIcon::None).unwrap_err();
        println!("show_message_yes_no on the event-loop thread: {err}");
        // `show_message` doesn't block; the window appears in this iteration's `about_to_wait`.
        let options = XDialogOptions { title: "winit host".into(),
                                       main_instruction: "Event-loop thread".into(),
                                       message: "Show a progress dialog?".into(),
                                       icon: XDialogIcon::Information,
                                       icon_source: None,
                                       buttons: vec!["No".into(), "Yes".into()] };
        self.question = Some(show_message(options));
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
    let mut app = XDialogBuilder::new().into_host_app(App { window: None, question: None, progress: None }, move || {
                                           let _ = waker.send_event(UserEvent::XDialog);
                                       })
                                       .unwrap();
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || worker(proxy));
    event_loop.run_app(&mut app).unwrap();
}
