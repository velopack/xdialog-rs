//! Accessibility: every dialog window gets an AccessKit adapter (UIA on Windows, AT-SPI on Linux,
//! NSAccessibility on macOS, through `accesskit_winit`), fed with the tree egui builds.
//!
//! The adapter's handlers run on any thread (platform dependent). They only queue an
//! [`A11yEvent`] for their dialog and wake the event loop; the runtime drains the queue in
//! `about_to_wait` and hands the events to the dialog ([`super::dialog::Dialog::set_assistive_tech`],
//! [`super::dialog::Dialog::handle_events`]). This works the same in builder and host mode (no
//! winit user event is needed). egui builds the tree only while an assistive technology is
//! active: the initial tree request enables AccessKit output in the dialog's context, and the
//! next frame's full tree goes to the adapter (`update_if_active`).
//!
//! What the tree contains comes from core: the root node is the dialog ([`describe_root`]),
//! [`super::theme::ButtonInteraction`] labels the buttons, [`super::text::TextBlockWidget`] the
//! texts; themes describe their progress bar and icon with [`describe_progress`] and
//! [`describe_icon`].

use std::sync::{Arc, Mutex};

use egui::accesskit::{self, ActionRequest, Role, TreeUpdate};
use egui::{Response, WidgetInfo, WidgetType};
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use super::theme::ProgressView;
use crate::channel::Inbox;
use crate::model::XDialogIcon;

/// A request from a platform adapter.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum A11yEvent {
    /// An assistive technology wants the tree (egui output on).
    Activated,
    /// An assistive technology asks for an action (click, focus, ...).
    Action(ActionRequest),
    /// No assistive technology any more (egui output off).
    Deactivated,
}

/// Adapter requests of every dialog window of one runtime, queued until the event loop drains them.
#[derive(Clone)]
pub(crate) struct A11yQueue {
    events: Arc<Mutex<Vec<(usize, A11yEvent)>>>,
    inbox: Arc<Inbox>,
}

impl A11yQueue {
    /// A queue whose pushes wake the loop that serves `inbox`.
    pub(crate) fn new(inbox: Arc<Inbox>) -> Self {
        A11yQueue { events: Arc::default(), inbox }
    }

    fn push(&self, id: usize, ev: A11yEvent) {
        self.events.lock().unwrap_or_else(|e| e.into_inner()).push((id, ev));
        self.inbox.wake();
    }

    /// Every queued request, oldest first.
    pub(crate) fn take(&self) -> Vec<(usize, A11yEvent)> {
        std::mem::take(&mut *self.events.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// The adapter of dialog `id`'s window. Must be created while the window is still invisible.
    pub(crate) fn adapter(&self, el: &ActiveEventLoop, window: &Window, id: usize) -> egui_winit::accesskit_winit::Adapter {
        let handler = || Handler { queue: self.clone(), id };
        egui_winit::accesskit_winit::Adapter::with_direct_handlers(el, window, handler(), handler(), handler())
    }
}

/// The adapter handlers of one dialog (any thread).
struct Handler {
    queue: A11yQueue,
    id: usize,
}

impl accesskit::ActivationHandler for Handler {
    /// No tree yet: egui builds it in the next frame (the adapter uses a placeholder until then).
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.queue.push(self.id, A11yEvent::Activated);
        None
    }
}

impl accesskit::ActionHandler for Handler {
    fn do_action(&mut self, request: ActionRequest) {
        self.queue.push(self.id, A11yEvent::Action(request));
    }
}

impl accesskit::DeactivationHandler for Handler {
    fn deactivate_accessibility(&mut self) {
        self.queue.push(self.id, A11yEvent::Deactivated);
    }
}

/// The root node: a dialog (an alert dialog for messages, so screen readers announce it) named
/// after the window title (the heading when there is none), described by the heading and body.
/// Call inside the pass; no-op while AccessKit output is off.
pub(crate) fn describe_root(ctx: &egui::Context, title: &str, heading: &str, body: &str, progress: bool) {
    ctx.accesskit_node_builder(egui::accesskit_root_id(), |n| {
           n.set_role(if progress { Role::Dialog } else { Role::AlertDialog });
           let label = if title.trim().is_empty() { heading } else { title };
           if !label.is_empty() {
               n.set_label(label);
           }
           let description = [heading, body].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n");
           if !description.is_empty() {
               n.set_description(description);
           }
       });
}

/// Describe a progress bar widget: a progress indicator with the target value in percent
/// (none while indeterminate).
pub(crate) fn describe_progress(response: &Response, progress: ProgressView) {
    let value = match progress {
        ProgressView::Determinate { value } => Some((value.clamp(0.0, 1.0) as f64 * 100.0).round()),
        ProgressView::Indeterminate { .. } => None,
    };
    response.widget_info(|| WidgetInfo { value, ..WidgetInfo::new(WidgetType::ProgressIndicator) });
    response.ctx.accesskit_node_builder(response.id, |n| {
                    if let Some(v) = value {
                        n.set_min_numeric_value(0.0);
                        n.set_max_numeric_value(100.0);
                        n.set_value(format!("{v}%"));
                    }
                });
}

/// Describe a severity icon widget as the severity word ("Information", "Warning", "Error"; nothing
/// for `None`, and for `Custom`, which is decorative). A text element, not an image: screen
/// readers read just the word, with no "image".
pub(crate) fn describe_icon(response: &Response, icon: &XDialogIcon) {
    let label = match icon {
        XDialogIcon::None | XDialogIcon::Custom => return,
        XDialogIcon::Information => "Information",
        XDialogIcon::Warning => "Warning",
        XDialogIcon::Error => "Error",
    };
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, label));
}
