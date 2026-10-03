//! The runtime of the drawn backends, shared by builder mode (`event_loop.rs`, xdialog owns the
//! winit loop) and host mode (`xdialog::host`, the application owns it): requests, dialog windows,
//! input, accessibility, repaint deadlines, the Win32 TaskDialog routing/fallback and panic
//! isolation.
//!
//! Window creation (`show`):
//! 1. `Dialog::new` checks fonts and runs the measure pass at the primary monitor's scale.
//! 2. An INVISIBLE window is created at exactly the measured (logical) size, with its icons; its
//!    AccessKit adapter (which must exist before the window is first shown) and drawing surface
//!    are attached (a different real scale re-requests the size).
//! 3. `set_visible(true)`, then the first frame is rendered and presented immediately.
//! 4. `reply.opened()`: the dialog's result goes to the caller from now on.
//!
//! If any of this fails or panics (text system, window, surface, first present), the request
//! (options, reply, callback) is still intact: with `fallback` (Windows `Auto`) it goes to a Win32
//! TaskDialog and the rest of the session uses TaskDialogs; otherwise the caller gets the error.
//!
//! Closed windows are hidden at once and dropped; their ids stay in `retired` until the loop
//! iteration in which winit reports `Destroyed` ends (X11 still delivers a redraw requested
//! before the close after it), so late events for them are still recognised as xdialog's.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowButtons, WindowId};

use super::a11y::A11y;
use super::appearance::{resolve_appearance, test_env_enabled};
use super::clock::DialogClock;
use super::dialog::{Dialog, DialogContent, DialogParams, Target};
use super::input::WinitInput;
use super::theme::{self, DialogKind};
use crate::backends::draw::{Text, TextSystem, WindowSurface, WindowTarget};
#[cfg(windows)]
use crate::backends::win32::TaskDialogManager;
#[cfg(windows)]
use crate::channel::DialogRequestHandler;
use crate::channel::{init_handler, Inbox, InboxHandler, UiThreadMark, WakeFn};
use crate::model::{DialogMessageRequest, DialogReply, XDialogBackend, XDialogOptions, XDialogResult, XDialogTheme};
use crate::{ProgressButtonCallback, XDialogError};

/// Height cap when no monitor is known, logical px.
const DEFAULT_MAX_HEIGHT: f64 = 800.0;
/// How long a dialog that needs fallback fonts waits for the system font scan when it opens.
const FONT_WAIT: Duration = Duration::from_millis(300);

struct DialogWindow {
    dialog: Dialog,
    /// `Rc`: the drawing surface holds a clone.
    window: Rc<Window>,
    input: WinitInput,
    a11y: A11y,
    /// Cached monitor refresh period and when it was read (re-read about once a second, so a
    /// window dragged to another monitor picks up its rate).
    period: Option<(Instant, Option<Duration>)>,
    /// Fully hidden (macOS, X11): frames are skipped until `Occluded(false)`.
    occluded: bool,
}

/// Every dialog of one event loop (builder or host). Dropping it closes them all (callers get
/// `WindowClosed`) and answers queued requests with `NoBackendAvailable` (later ones fail to send
/// and are answered the same way).
pub(crate) struct Runtime {
    /// Concrete backend (never `Auto`): a drawn theme, or `Win32` = TaskDialog routing.
    backend: XDialogBackend,
    /// A failing drawn dialog falls back to TaskDialogs (Windows `Auto`).
    #[cfg(windows)]
    fallback: bool,
    /// Created on first use (`Win32` routing or fallback).
    #[cfg(windows)]
    win32: Option<TaskDialogManager>,
    /// Light/dark override, re-resolved on appearance changes.
    xtheme: XDialogTheme,
    inbox: Arc<Inbox>,
    rx: Receiver<DialogMessageRequest>,
    /// A request received before the event loop existed (macOS builder mode: the first dialog
    /// request), handled before the queue.
    pending: Option<DialogMessageRequest>,
    dialogs: BTreeMap<usize, DialogWindow>,
    /// Windows of closed dialogs, until the iteration of their `Destroyed` ends (`true`: seen).
    retired: Vec<(WindowId, bool)>,
    /// The text system (fonts, layouts) every dialog of this runtime shares; created with the
    /// first dialog (warm factories make every later dialog's first frame fast).
    text: Option<Rc<Text>>,
    /// `ExitEventLoop` received (or the state can't be trusted after a double panic).
    pub(super) exit: bool,
    _ui_thread: UiThreadMark,
}

impl Runtime {
    /// Install the process's request handler (an inbox; `waker` makes the event loop iterate) and
    /// create the runtime that serves it. The current thread (the event-loop thread) becomes
    /// xdialog's UI thread. Fails if a handler is already installed.
    pub(crate) fn install(backend: XDialogBackend, fallback: bool, xtheme: XDialogTheme, waker: WakeFn) -> Result<Self, XDialogError> {
        #[cfg(not(windows))]
        let _ = fallback;
        let (inbox, rx) = Inbox::new(waker);
        if !init_handler(Box::new(InboxHandler(inbox.clone()))) {
            return Err(XDialogError::SystemError("a dialog backend is already initialized".into()));
        }
        #[cfg(draw_soft)]
        crate::backends::draw::start_font_scan();
        Ok(Runtime { backend,
                     #[cfg(windows)]
                     fallback,
                     #[cfg(windows)]
                     win32: None,
                     xtheme,
                     inbox,
                     rx,
                     pending: None,
                     dialogs: BTreeMap::new(),
                     retired: Vec::new(),
                     text: None,
                     exit: false,
                     _ui_thread: UiThreadMark::set() })
    }

    /// macOS builder mode, before the event loop exists: block until the first dialog request and
    /// return it (`None`: `ExitEventLoop` or every sender is gone). Requests for dialogs that were
    /// never created are no-ops, as they are once the loop runs.
    #[cfg(target_os = "macos")]
    pub(crate) fn wait_for_first_dialog(&mut self) -> Option<DialogMessageRequest> {
        loop {
            match self.rx.recv().ok()? {
                DialogMessageRequest::ExitEventLoop => return None,
                msg @ (DialogMessageRequest::ShowMessageWindow(..) | DialogMessageRequest::ShowProgressWindow(..)) => return Some(msg),
                _ => {}
            }
        }
    }

    /// macOS builder mode: the event loop now exists; `first` (from [`Self::wait_for_first_dialog`])
    /// is handled in its first iteration, `waker` wakes it from then on.
    #[cfg(target_os = "macos")]
    pub(crate) fn attach(&mut self, first: DialogMessageRequest, waker: WakeFn) {
        self.pending = Some(first);
        self.inbox.set_waker(waker);
    }

    /// Handle queued requests, refresh after font/appearance changes, apply accessibility
    /// requests, request due redraws. Returns the next time an iteration is needed (`None` =
    /// idle).
    pub(crate) fn about_to_wait(&mut self, el: &ActiveEventLoop) -> Option<Instant> {
        self.retired.retain(|r| !r.1);
        // Cleared before draining: a request sent from now on wakes the loop again.
        self.inbox.begin_drain();
        let mut refresh = false;
        while let Some(msg) = self.pending.take().or_else(|| self.rx.try_recv().ok()) {
            match msg {
                // Stopped (builder: the loop is ending; host: after a double panic): answer, don't
                // leave callers waiting.
                msg if self.exit => crate::channel::reject(msg),
                DialogMessageRequest::None => refresh = true,
                msg => {
                    let id = request_id(&msg);
                    self.guarded(id, "handling a dialog request", |rt| rt.handle_request(el, msg));
                }
            }
        }
        if refresh {
            self.guarded(None, "refreshing fonts/appearance", Self::refresh);
        }

        // Accessibility: activation / deactivation and requests queued by the AccessKit handlers.
        let keys: Vec<usize> = self.dialogs.iter().filter(|(_, w)| w.a11y.take_dirty()).map(|(k, _)| *k).collect();
        for key in keys {
            self.guarded(Some(key), "handling an accessibility request", |rt| {
                    let requests = rt.dialogs.get(&key).map(|w| w.a11y.take_requests()).unwrap_or_default();
                    for r in requests {
                        if let Some(w) = rt.dialogs.get_mut(&key) {
                            w.dialog.a11y_request(r);
                        }
                    }
                    rt.after(key);
                });
        }

        let now = Instant::now();
        let mut next: Option<Instant> = None;
        for w in self.dialogs.values_mut() {
            let s = w.dialog.schedule_mut();
            if s.due(now) {
                s.fired();
                w.window.request_redraw(); // its frame publishes the tree
            } else {
                if let Some(t) = s.next {
                    next = Some(next.map_or(t, |n| n.min(t)));
                }
                w.publish_a11y();
            }
        }
        next
    }

    /// Handle a window event. `true` if `id` is (or was, until its `Destroyed`) a dialog window.
    pub(crate) fn window_event(&mut self, id: WindowId, ev: &WindowEvent) -> bool {
        if let Some(r) = self.retired.iter_mut().find(|r| r.0 == id) {
            r.1 |= matches!(ev, WindowEvent::Destroyed);
            return true;
        }
        let Some(key) = self.dialogs.iter().find(|(_, w)| w.window.id() == id).map(|(k, _)| *k) else { return false };
        self.guarded(Some(key), "handling a window event", |rt| rt.dialog_event(key, ev));
        true
    }

    /// Every dialog window (open, or closed) with whether its `Destroyed` was already seen.
    #[cfg(feature = "winit-host")]
    pub(crate) fn window_ids(&self) -> Vec<(WindowId, bool)> {
        self.dialogs.values().map(|w| (w.window.id(), false)).chain(self.retired.iter().copied()).collect()
    }

    /// Host mode, a run of the loop ended: the next request wakes the loop again (the exiting
    /// iteration never drained, so `close_all`'s notifications left a wake outstanding).
    #[cfg(feature = "winit-host")]
    pub(crate) fn rearm_wake(&self) {
        self.inbox.begin_drain();
    }

    fn dialog_event(&mut self, key: usize, ev: &WindowEvent) {
        // No title bar (Windows; the macOS look): a primary press on the background moves the
        // window.
        let drag_by_background = cfg!(windows) || self.backend == XDialogBackend::MacOS;
        let Some(w) = self.dialogs.get_mut(&key) else { return };
        // Every event reaches the adapter (window bounds, focus).
        w.a11y.process_event(&w.window, ev);
        match ev {
            WindowEvent::RedrawRequested => w.redraw(),
            WindowEvent::Occluded(occluded) => {
                // A window placed by `XDIALOG_TEST_POS` may be off every monitor, which X11 reports as occluded.
                w.occluded = *occluded && test_position_raw().is_none();
                if !occluded {
                    w.window.request_redraw(); // the schedule went idle while hidden
                }
            }
            WindowEvent::Resized(s) => w.dialog.resized([s.width, s.height]),
            // winit keeps the logical size itself; the following `Resized` has the new size.
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => w.dialog.scale_changed(*scale_factor),
            WindowEvent::CloseRequested => w.dialog.finish(XDialogResult::WindowClosed),
            WindowEvent::ThemeChanged(_) => w.dialog.refresh_appearance(),
            _ => {
                let events = w.input.translate(ev, w.window.scale_factor());
                let drag = drag_by_background && events.iter().any(|e| {
                                           matches!(e, super::input::Event::PointerButton { pos, button: super::input::PointerButton::Primary, pressed: true }
                                                    if !w.dialog.hits_widget(*pos))
                                       });
                if !events.is_empty() {
                    w.dialog.handle_events(events);
                }
                if drag {
                    let _ = w.window.drag_window();
                }
            }
        }
        self.after(key);
    }

    fn handle_request(&mut self, el: &ActiveEventLoop, msg: DialogMessageRequest) {
        match msg {
            DialogMessageRequest::None => {}
            DialogMessageRequest::ExitEventLoop => {
                self.close_all();
                self.exit = true;
            }
            DialogMessageRequest::ShowMessageWindow(id, options, reply) => self.show(el, id, DialogKind::Message, options, reply, None),
            DialogMessageRequest::ShowProgressWindow(id, options, reply, callback) => {
                self.show(el, id, DialogKind::Progress, options, reply, callback)
            }
            DialogMessageRequest::CloseWindow(id)
            | DialogMessageRequest::SetProgressIndeterminate(id)
            | DialogMessageRequest::SetProgressValue(id, _)
            | DialogMessageRequest::SetProgressText(id, _) => {
                let Some(w) = self.dialogs.get_mut(&id) else {
                    // Not a drawn dialog: a TaskDialog (Win32 routing / fallback), if any.
                    #[cfg(windows)]
                    if let Some(m) = &self.win32 {
                        let _ = m.send(msg);
                    }
                    return;
                };
                match msg {
                    DialogMessageRequest::SetProgressIndeterminate(_) => w.dialog.set_progress_indeterminate(),
                    DialogMessageRequest::SetProgressValue(_, value) => w.dialog.set_progress_value(value),
                    DialogMessageRequest::SetProgressText(_, text) => w.dialog.set_text(&text),
                    _ => w.dialog.finish(XDialogResult::WindowClosed),
                }
                self.after(id);
            }
        }
    }

    fn show(&mut self,
            el: &ActiveEventLoop,
            id: usize,
            kind: DialogKind,
            options: XDialogOptions,
            reply: DialogReply,
            mut callback: Option<ProgressButtonCallback>) {
        #[cfg(windows)]
        if self.backend == XDialogBackend::Win32 {
            self.win32().show(id, options, kind == DialogKind::Progress, reply, callback);
            return;
        }
        if self.dialogs.contains_key(&id) {
            reply.failed(XDialogError::SystemError(format!("dialog id {id} already exists")));
            return;
        }
        // A panic while building the dialog or in its first frame is a failure like any other:
        // the request is still intact here.
        let shown = catch_unwind(AssertUnwindSafe(|| self.try_show(el, id, kind, &options, &mut callback)))
            .unwrap_or_else(|_| Err(XDialogError::SystemError("building the dialog panicked".into())));
        match shown {
            Ok(()) => {
                if let Some(w) = self.dialogs.get_mut(&id) {
                    w.dialog.set_sender(reply.opened());
                }
                self.after(id);
            }
            Err(e) => {
                #[cfg(windows)]
                if self.fallback {
                    warn!("xdialog: {e}; using Win32 TaskDialog for the rest of the session");
                    self.backend = XDialogBackend::Win32;
                    self.win32().show(id, options, kind == DialogKind::Progress, reply, callback);
                    return;
                }
                error!("xdialog: could not show dialog {id}: {e}");
                reply.failed(e);
            }
        }
    }

    fn text(&mut self) -> Result<Rc<Text>, XDialogError> {
        if let Some(t) = &self.text {
            return Ok(t.clone());
        }
        let text = Text::shared().map_err(|e| XDialogError::SystemError(format!("no text system: {e}")))?;
        Ok(self.text.insert(text).clone())
    }

    /// Build dialog `id` and its window, show it with its first frame and add it to `dialogs`.
    /// Borrows the request so that a failure leaves it intact for the caller; `callback` is taken
    /// only once nothing can fail any more.
    fn try_show(&mut self,
                el: &ActiveEventLoop,
                id: usize,
                kind: DialogKind,
                options: &XDialogOptions,
                callback: &mut Option<ProgressButtonCallback>)
                -> Result<(), XDialogError> {
        // Test builds: fail like a broken drawing backend would (the fallback tests).
        if super::appearance::test_flag("XDIALOG_TEST_FAIL_DRAWN") {
            return Err(XDialogError::SystemError("XDIALOG_TEST_FAIL_DRAWN is set".into()));
        }
        let text = self.text()?;
        let primary = el.primary_monitor().or_else(|| el.available_monitors().next());
        let ppp = primary.as_ref().map_or(1.0, |m| m.scale_factor());
        let max_height = primary.as_ref()
                                .map(|m| m.size().height as f64 / m.scale_factor() * 0.9)
                                .filter(|h| h.is_finite() && *h > 0.0)
                                .unwrap_or(DEFAULT_MAX_HEIGHT);
        let params = DialogParams { id,
                                    content: DialogContent::new(kind, options.clone()),
                                    appearance: resolve_appearance(self.xtheme),
                                    system_appearance: Some(self.xtheme),
                                    ppp,
                                    max_height,
                                    clock: DialogClock::new(),
                                    sender: None,
                                    font_wait: FONT_WAIT,
                                    text: text.clone() };
        let mut dialog = Dialog::new(theme::new(self.backend), params);
        if no_activate() {
            // Never activated: unfocused from the first frame.
            dialog.handle_events([super::input::Event::WindowFocused(false)]);
        }

        let size = dialog.desired_size();
        let dark = dialog.dark_titlebar();
        let mut attrs = Window::default_attributes().with_title(dialog.title())
                                                    .with_inner_size(LogicalSize::new(size.width, size.height))
                                                    .with_resizable(false)
                                                    .with_enabled_buttons(WindowButtons::CLOSE)
                                                    .with_visible(false)
                                                    .with_active(!no_activate())
                                                    .with_theme((!self.follows_system()).then_some(winit_theme(dark)));
        if let Some(file) = dialog.icon_file() {
            // Windows: the title bar (small) and taskbar / Alt+Tab (big) icons at the system
            // metrics' sizes; X11: one image the window manager scales (_NET_WM_ICON). Wayland and
            // macOS have no window icons.
            #[cfg(windows)]
            {
                use winit::platform::windows::WindowAttributesExtWindows;
                attrs = attrs.with_taskbar_icon(window_icon(file, 32.0 * ppp));
            }
            attrs = attrs.with_window_icon(window_icon(file, if cfg!(windows) { 16.0 } else { 64.0 } * ppp));
        }
        #[cfg(windows)]
        {
            // No title bar or close button: the dialog draws its whole client area and is moved by
            // dragging its background (`dialog_event`). The title still names the window in the
            // taskbar, Alt+Tab and to screen readers; Alt+F4 still closes it (WS_SYSMENU stays).
            // DWM shadow and Windows 11 rounded corners keep it looking like a window. No drag and
            // drop: winit's default registers an OLE drop target, which needs (and on an
            // uninitialised host thread silently makes) an STA thread and aborts on an MTA one.
            use winit::platform::windows::{CornerPreference, WindowAttributesExtWindows};
            attrs = attrs.with_decorations(false)
                         .with_undecorated_shadow(true)
                         .with_drag_and_drop(false)
                         .with_corner_preference(CornerPreference::Round);
        }
        // The macOS look: the title bar hidden (its buttons too) under a full-size content view, so
        // the dialog draws the whole window and AppKit still gives it rounded corners and a
        // shadow; transparent over the alert material when the theme is translucent.
        #[cfg(target_os = "macos")]
        let translucent = self.backend == XDialogBackend::MacOS && dialog.wants_translucency();
        #[cfg(target_os = "macos")]
        if self.backend == XDialogBackend::MacOS {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs = attrs.with_titlebar_transparent(true)
                         .with_title_hidden(true)
                         .with_titlebar_buttons_hidden(true)
                         .with_fullsize_content_view(true)
                         .with_transparent(translucent);
        }
        let px = dialog.physical_size(ppp);
        let virtual_left = || el.available_monitors().map(|m| m.position().x).min().unwrap_or(0);
        let position = test_position(px, virtual_left).or_else(|| {
                                                          // Centred on the primary monitor (physical), computed up front so the
                                                          // window manager doesn't reposition it after mapping.
                                                          let m = primary.as_ref()?;
                                                          let (msize, mpos) = (m.size(), m.position());
                                                          // macOS alerts sit higher: a third of the free space above.
                                                          let above = if self.backend == XDialogBackend::MacOS { 3 } else { 2 };
                                                          Some([mpos.x + (msize.width as i32 - px[0] as i32) / 2,
                                                                mpos.y + (msize.height as i32 - px[1] as i32) / above])
                                                      });
        if let Some([x, y]) = position {
            attrs = attrs.with_position(PhysicalPosition::new(x, y));
        }
        let window = Rc::new(el.create_window(attrs).map_err(|e| XDialogError::SystemError(format!("could not create a window: {e}")))?);
        // Until the dialog is complete: a failure (or panic) below drops the window, and its late
        // `Destroyed` must still be recognised as xdialog's.
        self.retired.push((window.id(), false));
        #[cfg(windows)]
        super::platform_win::set_dark_titlebar(&window, dark);

        // AccessKit needs its adapter before the window is first shown.
        let inbox = self.inbox.clone();
        let a11y = A11y::new(el, &window, Box::new(move || inbox.wake()));
        #[cfg(target_os = "macos")]
        let translucent = translucent && super::platform_mac::add_material(&window, dialog.window_material());
        #[cfg(target_os = "macos")]
        let surface = if translucent { WindowSurface::translucent(&window, &text) } else { WindowSurface::new(&window, &text) };
        #[cfg(not(target_os = "macos"))]
        let surface = WindowSurface::new(&window, &text);
        let surface = surface.map_err(|e| XDialogError::SystemError(format!("could not create a surface: {e}")))?;
        #[cfg(target_os = "macos")]
        dialog.set_translucent(translucent);
        let s = window.inner_size();
        dialog.attach(Target::Window(surface), window.scale_factor(), [s.width, s.height]);
        window.set_visible(true);
        // Present right away: presenting to a hidden window is a no-op on Win32/X11 and the class
        // background would flash.
        dialog.frame().map_err(|e| XDialogError::SystemError(format!("could not present: {e}")))?;
        #[cfg(target_os = "macos")]
        if translucent {
            super::platform_mac::invalidate_shadow(&window);
        }
        dialog.set_callback(callback.take());
        self.retired.retain(|r| r.0 != window.id());
        self.dialogs.insert(id, DialogWindow { dialog, window, input: WinitInput::default(), a11y, period: None, occluded: false });
        Ok(())
    }

    /// Whether dialog windows are created without a preferred theme, so that winit reports the
    /// system's light/dark switches as `ThemeChanged`: Windows and macOS only report them for such
    /// windows (the macOS delegate ignores `effectiveAppearance` changes once an appearance is
    /// pinned, Windows never sends them to a themed window). Elsewhere the portal reports them.
    fn follows_system(&self) -> bool {
        cfg!(any(windows, target_os = "macos")) && self.xtheme == XDialogTheme::SystemDefault
    }

    /// Hide `window` now and remember its id until winit reports it destroyed (the caller drops it).
    fn retire(&mut self, window: &Window) {
        window.set_visible(false);
        self.retired.push((window.id(), false));
    }

    #[cfg(windows)]
    fn win32(&mut self) -> &TaskDialogManager {
        self.win32.get_or_insert_with(TaskDialogManager::new)
    }

    /// Post-processing after anything touched dialog `key`: remove a closed dialog (hide, drop
    /// surface and window), forward resize / title-bar requests. The accessibility tree is
    /// published once per iteration (`about_to_wait`) or frame, not here.
    fn after(&mut self, key: usize) {
        #[cfg(not(windows))]
        let follows_system = self.follows_system();
        let Some(w) = self.dialogs.get_mut(&key) else { return };
        if w.dialog.is_closed() {
            let w = self.dialogs.remove(&key).expect("present");
            self.retire(&w.window);
            return;
        }
        if let Some(size) = w.dialog.take_resize() {
            // `Some`: applied at once, and winit may not emit `Resized` for it (Wayland).
            if let Some(s) = w.window.request_inner_size(LogicalSize::new(size.width, size.height)) {
                w.dialog.resized([s.width, s.height]);
            }
        }
        if let Some(dark) = w.dialog.take_titlebar_change() {
            // Windows: DWM directly (`Window::set_theme` doesn't change winit's preferred theme,
            // and `ThemeChanged` must keep flowing for system-following dialogs). A
            // system-following window elsewhere keeps its appearance unpinned for the same reason.
            #[cfg(windows)]
            super::platform_win::set_dark_titlebar(&w.window, dark);
            #[cfg(not(windows))]
            if !follows_system {
                w.window.set_theme(Some(winit_theme(dark)));
            }
        }
    }

    /// Fonts or the appearance may have changed (a background thread woke the loop).
    fn refresh(&mut self) {
        let keys: Vec<usize> = self.dialogs.keys().copied().collect();
        for key in keys {
            if let Some(w) = self.dialogs.get_mut(&key) {
                w.dialog.refresh_fonts();
                w.dialog.refresh_appearance();
            }
            self.after(key);
        }
    }

    /// Close every dialog, drawn (`WindowClosed`, windows hidden and dropped) and TaskDialog. The
    /// runtime keeps serving (host mode: a run of the loop ended, the next one shows the queued
    /// requests).
    pub(crate) fn close_all(&mut self) {
        for (_, mut w) in std::mem::take(&mut self.dialogs) {
            w.dialog.finish(XDialogResult::WindowClosed);
            self.retire(&w.window);
        }
        #[cfg(windows)]
        if let Some(m) = &self.win32 {
            m.close_all();
        }
    }

    /// Run `f` under `catch_unwind`: a panic closes only the affected dialog `id` (if known); if
    /// closing it panics too, the state can't be trusted any more and the loop should exit.
    fn guarded(&mut self, id: Option<usize>, what: &str, f: impl FnOnce(&mut Self)) {
        if catch_unwind(AssertUnwindSafe(|| f(self))).is_ok() {
            return;
        }
        error!("xdialog: {what} panicked");
        let Some(id) = id else { return };
        let closed = catch_unwind(AssertUnwindSafe(|| {
                                      if let Some(w) = self.dialogs.get_mut(&id) {
                                          w.dialog.finish(XDialogResult::WindowClosed);
                                      }
                                      self.after(id);
                                  }));
        if closed.is_err() {
            error!("xdialog: closing dialog {id} after a panic panicked again; stopping");
            self.exit = true;
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.close_all();
        self.inbox.close();
        self.pending.take().into_iter().chain(self.rx.try_iter()).for_each(crate::channel::reject);
    }
}

impl DialogWindow {
    /// Render a frame (`RedrawRequested`). Nothing is drawn while the window is occluded or has no
    /// area (minimised): the schedule was cleared when this redraw was requested, so the dialog
    /// idles until `Occluded(false)` / `Resized` ask for a frame again.
    fn redraw(&mut self) {
        let size = self.window.inner_size();
        if self.occluded || size.width == 0 || size.height == 0 {
            // The frame that reveals the window must present.
            self.dialog.schedule_mut().skipped();
            return;
        }
        let period = self.monitor_period();
        self.dialog.schedule_mut().set_monitor_period(period);
        // Wayland: requests the frame callback that throttles redraws of a hidden surface. Only
        // before an actual present: winit withholds redraws until the callback, which only a
        // commit brings.
        let window = &self.window;
        if let Err(e) = self.dialog.frame_with(|| window.pre_present_notify()) {
            warn!("xdialog: present failed: {e}");
        }
        self.publish_a11y();
    }

    /// Publish the accessibility tree (a no-op without an assistive technology, or unchanged).
    fn publish_a11y(&mut self) {
        let dialog = &self.dialog;
        self.a11y.update(|| dialog.a11y_tree());
    }

    /// Refresh period of the monitor showing the window; `None` when unknown (60 Hz is assumed).
    fn monitor_period(&mut self) -> Option<Duration> {
        let now = Instant::now();
        if let Some((at, period)) = self.period {
            if now.duration_since(at) < Duration::from_secs(1) {
                return period;
            }
        }
        let period = self.window
                         .current_monitor()
                         .and_then(|m| m.refresh_rate_millihertz())
                         .filter(|&mhz| mhz > 0)
                         .map(|mhz| Duration::from_secs_f64(1000.0 / mhz as f64));
        self.period = Some((now, period));
        period
    }
}

/// Fonts found by the background scan or the system appearance changed (any thread): the runtime
/// (there is at most one per process: its inbox is the installed handler) wakes up and refreshes
/// its dialogs. Every other handler ignores the request.
#[cfg(draw_soft)]
pub(crate) fn wake_all() {
    let _ = crate::channel::send_request(DialogMessageRequest::None);
}

/// The dialog a request concerns (closed after a panic while handling it), if any.
fn request_id(msg: &DialogMessageRequest) -> Option<usize> {
    match msg {
        DialogMessageRequest::ShowMessageWindow(id, ..)
        | DialogMessageRequest::ShowProgressWindow(id, ..)
        | DialogMessageRequest::CloseWindow(id)
        | DialogMessageRequest::SetProgressIndeterminate(id)
        | DialogMessageRequest::SetProgressValue(id, _)
        | DialogMessageRequest::SetProgressText(id, _) => Some(*id),
        DialogMessageRequest::None | DialogMessageRequest::ExitEventLoop => None,
    }
}

/// `file` as a winit window icon of `size` physical px.
fn window_icon(file: &crate::icon::IconFile, size: f64) -> Option<winit::window::Icon> {
    let img = file.render(size.round() as u32)?;
    winit::window::Icon::from_rgba(img.rgba, img.size, img.size).map_err(|e| warn!("xdialog: window icon: {e}")).ok()
}

fn winit_theme(dark: bool) -> winit::window::Theme {
    if dark {
        winit::window::Theme::Dark
    } else {
        winit::window::Theme::Light
    }
}

/// `XDIALOG_TEST_NO_ACTIVATE=1`: never activate/focus dialog windows. Honoured in all builds.
pub(super) fn no_activate() -> bool {
    std::env::var_os("XDIALOG_TEST_NO_ACTIVATE").is_some_and(|v| !v.is_empty() && v != "0")
}

/// `XDIALOG_TEST_POS`, when test env vars are honoured.
fn test_position_raw() -> Option<String> {
    if !test_env_enabled() {
        return None;
    }
    std::env::var("XDIALOG_TEST_POS").ok()
}

/// `XDIALOG_TEST_POS=x,y|offscreen` (test builds only): physical top-left of a new window of
/// `size_px`; `virtual_left` is the left edge of the whole virtual desktop (physical px).
fn test_position(size_px: [u32; 2], virtual_left: impl FnOnce() -> i32) -> Option<[i32; 2]> {
    let v = test_position_raw()?;
    let v = v.trim();
    if v.eq_ignore_ascii_case("offscreen") {
        // Left of the whole virtual desktop: never on a real monitor.
        return Some([virtual_left() - size_px[0] as i32 - 64, 64]);
    }
    let (x, y) = v.split_once(',')?;
    Some([x.trim().parse().ok()?, y.trim().parse().ok()?])
}

// -------------------------------------------------------------------------------------------------
// Test hooks (`XDialogHost::test_*`)
// -------------------------------------------------------------------------------------------------

#[cfg(all(feature = "winit-host", feature = "_test-hooks"))]
impl Runtime {
    pub(crate) fn test_dialogs(&self) -> Vec<crate::host::LiveDialog> {
        self.dialogs
            .iter()
            .map(|(&id, w)| crate::host::LiveDialog { id,
                                                      title: w.dialog.title().to_owned(),
                                                      button_rects: w.dialog.button_rects(),
                                                      frames: w.dialog.frames(),
                                                      a11y: super::a11y::dump(&w.dialog.a11y_tree()) })
            .collect()
    }

    pub(crate) fn test_inject(&mut self, id: usize, ev: super::input::Event) {
        if let Some(w) = self.dialogs.get_mut(&id) {
            w.dialog.handle_events([ev]);
            self.after(id);
        }
    }
}
