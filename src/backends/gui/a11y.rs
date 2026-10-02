//! Accessibility: every dialog window gets an AccessKit adapter (UIA on Windows, AT-SPI on Linux,
//! NSAccessibility on macOS, through `accesskit_winit`), fed with the tree of [`tree`].
//!
//! ```text
//! dialog (AlertDialog for messages | Dialog for progress; label = title, or the heading when
//! │       the title is blank; description = heading + "\n" + body)
//! ├── heading (Label, value = the heading)                 if any
//! ├── icon (Label, value = "Information" | "Warning" | "Error"; none for None and Custom)
//! ├── body (Label, value = the body)                       if any
//! ├── progress (ProgressIndicator, 0..100 and "{v}%"; no value while indeterminate)
//! └── buttons (Button, Click + Focus), Tab order
//! ```
//!
//! The tree is built on demand: only while an assistive technology is active (the activation
//! handler set the `active` flag, the deactivation handler cleared it) and published only when it
//! changed. The adapter's handlers may run on any thread: they only update the shared state, queue
//! action requests and wake the event loop, which applies them on the UI thread through the
//! keyboard paths (`Dialog::a11y_request`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use accesskit::{Action, ActionRequest, Node, NodeId, Rect as AkRect, Role, TreeId, TreeInfo, TreeUpdate};

use super::dialog::DialogContent;
use super::keyboard::focused_button;
use super::theme::{DialogKind, DialogUiOutput, ProgressView};
use crate::backends::draw::{Point, Rect};
use crate::model::XDialogIcon;

const ROOT: NodeId = NodeId(1);
const HEADING: NodeId = NodeId(2);
const ICON: NodeId = NodeId(3);
const BODY: NodeId = NodeId(4);
const PROGRESS: NodeId = NodeId(5);
const FIRST_BUTTON: u64 = 100;

/// The node of button `index` (API index).
pub(crate) fn button_node(index: usize) -> NodeId {
    NodeId(FIRST_BUTTON + index as u64)
}

/// The API index of a button node.
pub(crate) fn node_button(node: NodeId) -> Option<usize> {
    node.0.checked_sub(FIRST_BUTTON).map(|i| i as usize)
}

/// What the tree is built from.
pub(crate) struct TreeSource<'a> {
    pub content: &'a DialogContent,
    pub out: &'a DialogUiOutput,
    pub focus: Option<usize>,
    /// Physical px per logical px (bounds are physical, client-relative).
    pub ppp: f64,
}

/// The full tree of a dialog.
pub(crate) fn tree(s: &TreeSource<'_>) -> TreeUpdate {
    let c = s.content;
    let bounds = |r: Rect| {
        let r = r.scale(s.ppp);
        AkRect { x0: r.x0, y0: r.y0, x1: r.x1, y1: r.y1 }
    };
    let mut nodes = Vec::new();
    let mut children = Vec::new();
    let mut add = |id: NodeId, node: Node, nodes: &mut Vec<(NodeId, Node)>| {
        children.push(id);
        nodes.push((id, node));
    };
    // A text element: screen readers read the value, with no "image" or other role noise.
    let label = |value: &str, rect: Option<Rect>| {
        let mut n = Node::new(Role::Label);
        n.set_value(value);
        if let Some(r) = rect {
            n.set_bounds(bounds(r));
        }
        n
    };
    if !c.heading.is_empty() {
        add(HEADING, label(&c.heading, s.out.parts.heading), &mut nodes);
    }
    let icon = match c.icon {
        // `Custom` is decorative.
        XDialogIcon::None | XDialogIcon::Custom => None,
        XDialogIcon::Information => Some("Information"),
        XDialogIcon::Warning => Some("Warning"),
        XDialogIcon::Error => Some("Error"),
    };
    if let Some(word) = icon {
        add(ICON, label(word, s.out.parts.icon), &mut nodes);
    }
    if !c.body.is_empty() {
        add(BODY, label(&c.body, s.out.parts.body), &mut nodes);
    }
    if let Some(p) = c.progress {
        let mut n = Node::new(Role::ProgressIndicator);
        if let ProgressView::Determinate { value } = p {
            let v = (value.clamp(0.0, 1.0) as f64 * 100.0).round();
            n.set_min_numeric_value(0.0);
            n.set_max_numeric_value(100.0);
            n.set_numeric_value(v);
            n.set_value(format!("{v}%"));
        }
        if let Some(r) = s.out.parts.progress {
            n.set_bounds(bounds(r));
        }
        add(PROGRESS, n, &mut nodes);
    }
    for b in &s.out.buttons {
        let mut n = Node::new(Role::Button);
        n.set_label(c.buttons.get(b.index).map_or("", String::as_str));
        n.add_action(Action::Click);
        n.add_action(Action::Focus);
        n.set_bounds(bounds(b.rect));
        add(button_node(b.index), n, &mut nodes);
    }
    let mut root = Node::new(if c.kind == DialogKind::Message { Role::AlertDialog } else { Role::Dialog });
    let name = if c.title.trim().is_empty() { c.heading.as_str() } else { c.title.as_str() };
    if !name.is_empty() {
        root.set_label(name);
    }
    let description = [c.heading.as_str(), c.body.as_str()].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n");
    if !description.is_empty() {
        root.set_description(description);
    }
    root.set_bounds(bounds(Rect::from_origin_size(Point::ZERO, s.out.desired_size)));
    root.set_children(children);
    nodes.insert(0, (ROOT, root));
    let focus = focused_button(s.focus, s.out).map_or(ROOT, button_node);
    TreeUpdate { nodes, tree: Some(TreeInfo::new(ROOT)), tree_id: TreeId::ROOT, focus }
}

/// A screen-reader request for the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    /// Activate button (API index), like a click.
    Click(usize),
    /// Move keyboard focus to button (API index).
    Focus(usize),
}

impl Request {
    pub(crate) fn from_action(req: &ActionRequest) -> Option<Request> {
        let b = node_button(req.target_node)?;
        match req.action {
            Action::Click => Some(Request::Click(b)),
            Action::Focus => Some(Request::Focus(b)),
            _ => None,
        }
    }
}

/// State shared with the AccessKit handlers (any thread).
struct Shared {
    /// An assistive technology asked for the tree and has not gone away.
    active: AtomicBool,
    /// Something changed for the UI thread (activation, deactivation, requests).
    dirty: AtomicBool,
    /// The adapter asked for an initial tree: the next update publishes in full even when the
    /// tree is unchanged (a deactivation and reactivation may both land between two updates).
    reset: AtomicBool,
    requests: Mutex<Vec<Request>>,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl Shared {
    fn notify(&self) {
        self.dirty.store(true, Ordering::SeqCst);
        (self.wake)();
    }
}

struct Handler(Arc<Shared>);

impl accesskit::ActivationHandler for Handler {
    /// No tree yet: the UI thread builds it in its next iteration and publishes it (the adapter
    /// uses a placeholder until then).
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.0.active.store(true, Ordering::SeqCst);
        self.0.reset.store(true, Ordering::SeqCst);
        self.0.notify();
        None
    }
}

impl accesskit::ActionHandler for Handler {
    fn do_action(&mut self, req: ActionRequest) {
        if let Some(r) = Request::from_action(&req) {
            self.0.requests.lock().unwrap_or_else(|e| e.into_inner()).push(r);
            self.0.notify();
        }
    }
}

impl accesskit::DeactivationHandler for Handler {
    fn deactivate_accessibility(&mut self) {
        self.0.active.store(false, Ordering::SeqCst);
        self.0.notify();
    }
}

/// The AccessKit adapter of one dialog window.
pub(crate) struct A11y {
    adapter: accesskit_winit::Adapter,
    shared: Arc<Shared>,
    /// The tree last published (`None`: nothing published while active).
    last: Option<TreeUpdate>,
}

impl A11y {
    /// Create the adapter for `window`, which must not have been shown yet. `wake` makes the event
    /// loop iterate (queued changes are taken in `about_to_wait`).
    pub(crate) fn new(el: &winit::event_loop::ActiveEventLoop, window: &winit::window::Window, wake: Box<dyn Fn() + Send + Sync>) -> Self {
        let shared = Arc::new(Shared { active: AtomicBool::new(false),
                                       dirty: AtomicBool::new(false),
                                       reset: AtomicBool::new(false),
                                       requests: Mutex::new(Vec::new()),
                                       wake });
        let handler = || Handler(shared.clone());
        let adapter = accesskit_winit::Adapter::with_direct_handlers(el, window, handler(), handler(), handler());
        A11y { adapter, shared, last: None }
    }

    /// Every window event goes to the adapter (window bounds, focus).
    pub(crate) fn process_event(&mut self, window: &winit::window::Window, ev: &winit::event::WindowEvent) {
        self.adapter.process_event(window, ev);
    }

    /// Whether the handlers changed something since the last call (the owner then takes the
    /// requests and publishes).
    pub(crate) fn take_dirty(&self) -> bool {
        self.shared.dirty.swap(false, Ordering::SeqCst)
    }

    /// Requests queued by the action handler.
    pub(crate) fn take_requests(&self) -> Vec<Request> {
        std::mem::take(&mut *self.shared.requests.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Publish the dialog's current tree while an assistive technology is active (built only
    /// then; a no-op when unchanged).
    pub(crate) fn update(&mut self, tree: impl FnOnce() -> TreeUpdate) {
        if self.shared.reset.swap(false, Ordering::SeqCst) {
            self.last = None;
        }
        if !self.shared.active.load(Ordering::SeqCst) {
            self.last = None;
            return;
        }
        let tree = tree();
        if self.last.as_ref() == Some(&tree) {
            return;
        }
        self.last = Some(tree.clone());
        self.adapter.update_if_active(|| tree);
    }
}

/// A readable dump of a tree (tests, diagnostics): one line per node, children indented.
#[cfg(feature = "_test-hooks")]
pub(crate) fn dump(t: &TreeUpdate) -> String {
    fn walk(t: &TreeUpdate, id: NodeId, depth: usize, out: &mut String) {
        let Some((_, n)) = t.nodes.iter().find(|(i, _)| *i == id) else { return };
        let mut line = format!("{}{:?}", "  ".repeat(depth), n.role());
        for (k, v) in [("label", n.label()), ("value", n.value()), ("description", n.description())] {
            if let Some(v) = v {
                line += &format!(" {k}={v:?}");
            }
        }
        if let Some(v) = n.numeric_value() {
            line += &format!(" numeric={v}");
        }
        for a in [Action::Click, Action::Focus] {
            if n.supports_action(a) {
                line += &format!(" +{a:?}");
            }
        }
        if let Some(b) = n.bounds() {
            line += &format!(" [{},{} {}x{}]", b.x0, b.y0, b.x1 - b.x0, b.y1 - b.y0);
        }
        if t.focus == id {
            line += " (focused)";
        }
        out.push_str(&line);
        out.push('\n');
        for c in n.children() {
            walk(t, *c, depth + 1, out);
        }
    }
    let mut out = String::new();
    walk(t, t.tree.as_ref().map_or(ROOT, |i| i.root), 0, &mut out);
    out
}
