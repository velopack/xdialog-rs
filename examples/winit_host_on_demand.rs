//! An application that runs its winit event loop several times (`run_app_on_demand`), with a new
//! app value for every run, and shows xdialog's dialogs in each of them.
//!
//! ```sh
//! cargo run --example winit_host_on_demand --features winit-host
//! ```
//!
//! One `XDialogHost` outlives the runs and wraps each run's app. Every run opens its own window:
//! click it to ask a question from the event-loop thread (Yes ends the run), close it to end the
//! run. A worker thread asks a question before the first run (queued, shown by the first run)
//! and, told by `main` each time a run has ended, another one (queued, shown by the next run; a
//! run that ends with a dialog open closes it with `WindowClosed`). After the last run the host
//! is dropped and dialog calls fail fast.

use std::sync::mpsc;

use xdialog::host::winit::application::ApplicationHandler;
use xdialog::host::winit::event::{ElementState, WindowEvent};
use xdialog::host::winit::event_loop::{ActiveEventLoop, EventLoop};
use xdialog::host::winit::platform::run_on_demand::EventLoopExtRunOnDemand;
use xdialog::host::winit::window::{Window, WindowId};
use xdialog::*;

const RUNS: u32 = 3;

/// One run of the loop. Nothing in it is xdialog-specific except the question it asks.
struct Run {
    n: u32,
    window: Option<Window>,
    /// The question asked from the event-loop thread, until answered.
    question: Option<MessageDialogProxy>,
}

impl ApplicationHandler for Run {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_none() {
            let title = format!("run {} of {} (click: ask, close: end the run)", self.n, RUNS);
            self.window = Some(el.create_window(Window::default_attributes().with_title(title)).unwrap());
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.window.as_ref().is_none_or(|w| w.id() != id) {
            return; // not ours (a window of an earlier run being destroyed, for instance)
        }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::MouseInput { state: ElementState::Pressed, .. } if self.question.is_none() => {
                // Non-blocking on the event-loop thread; the window appears in this iteration.
                let options = XDialogOptions { title: "winit host".into(),
                                               main_instruction: format!("Run {}", self.n),
                                               message: "End this run?".into(),
                                               icon: XDialogIcon::Information,
                                               buttons: vec!["No".into(), "Yes".into()],
                                               ..Default::default() };
                self.question = Some(show_message(options));
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // xdialog wakes the loop when the answer arrives.
        let Some(answer) = self.question.as_ref().and_then(|q| q.try_result()) else { return };
        self.question = None;
        if let Ok(XDialogResult::ButtonPressed(1)) = answer {
            el.exit();
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        println!("run {} ended", self.n);
    }
}

/// `ended` yields the number of each run once it has ended.
fn worker(ended: mpsc::Receiver<u32>) {
    let ask = |n: u32| {
        let text =
            if n == 0 { "Asked before the first run; shown by it.".to_owned() } else { format!("Asked after run {n}; shown by the next.") };
        println!("worker: {text}");
        let result = show_message_info_ok("winit host", "Worker thread", &text);
        println!("worker: {result:?}");
    };
    ask(0);
    for n in ended {
        // Blocks until the next run shows the dialog and the user (or that run's end) answers it.
        ask(n);
    }
}

fn main() {
    let mut event_loop = EventLoop::new().unwrap();
    let proxy = event_loop.create_proxy();
    let mut host = XDialogBuilder::new().into_host(move || {
                                            let _ = proxy.send_event(()); // the waker's event: `Run` ignores it
                                        })
                                        .unwrap();
    let (run_ended, ended) = mpsc::channel();
    let worker = std::thread::spawn(move || worker(ended));
    for n in 1..=RUNS {
        let mut run = Run { n, window: None, question: None };
        event_loop.run_app_on_demand(&mut host.wrap(&mut run)).unwrap();
        run_ended.send(n).unwrap();
    }
    // The end: dialogs close and dialog calls fail fast, so the worker's last question doesn't
    // wait for a run that never comes; closing the channel then ends its loop.
    drop(host);
    drop(run_ended);
    worker.join().unwrap();
}
