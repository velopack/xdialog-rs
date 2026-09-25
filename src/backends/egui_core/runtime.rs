//! The egui runtime, shared by builder mode (`event_loop.rs`, xdialog owns the winit loop) and
//! host mode (`xdialog::host`, the application owns it): requests, dialog windows, egui-winit
//! input, repaint deadlines, the Win32 TaskDialog routing/fallback and panic isolation.
//!
//! Window creation (`show`):
//! 1. `Dialog::new` builds the egui context (fonts before any pass) and runs the measure pass at
//!    the primary monitor's scale.
//! 2. An INVISIBLE window is created at exactly the measured (logical) size, the presenter and the
//!    egui-winit input state are attached (a different real scale re-requests the size).
//! 3. `set_visible(true)`, then the first frame is rendered and presented immediately.
//! 4. `creation.send(Ok(result_rx))`.
//!
//! If any of this fails or panics (window, softbuffer context, surface, first present), the
//! request (options, creation sender, callback) is still intact: with `fallback` (Windows `Auto`)
//! it goes to a Win32 TaskDialog and the rest of the session uses TaskDialogs; otherwise the caller
//! gets the error.
//!
//! Closed windows are hidden at once and dropped; their ids stay in `retired` until winit reports
//! `Destroyed`, so late events for them are still recognised as xdialog's.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::ViewportId;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, OwnedDisplayHandle};
use winit::window::{Window, WindowButtons, WindowId};

use super::appearance::{resolve_appearance, test_env_enabled};
use super::clock::DialogClock;
use super::dialog::{Dialog, DialogContent, DialogParams, MAX_TEXTURE_SIDE};
use super::fonts::FontRegistry;
use super::render::SoftwarePresenter;
use super::theme::{self, DialogKind};
use crate::channel::{init_handler, Inbox, InboxHandler, UiThreadMark, WakeFn};
#[cfg(windows)]
use crate::channel::DialogRequestHandler;
use crate::model::{CreationSender, DialogMessageRequest, XDialogBackend, XDialogOptions, XDialogResult, XDialogTheme};
use crate::{ProgressButtonCallback, XDialogError};

#[cfg(windows)]
use crate::backends::win32::TaskDialogManager;

/// Height cap when no monitor is known, logical px.
const DEFAULT_MAX_HEIGHT: f32 = 800.0;

/// One egui dialog and its window.
struct DialogWindow {
    dialog: Dialog,
    /// `Rc`: the softbuffer surface holds a clone.
    window: Rc<Window>,
    /// Translates winit events into `egui::Event`s (nothing else of egui-winit is used).
    input: egui_winit::State,
    /// Cached monitor refresh period and when it was read (re-read about once a second, so a
    /// window dragged to another monitor picks up its rate).
    period: Option<(Instant, Option<Duration>)>,
}

/// Every dialog of one event loop (builder or host). Dropping it closes them all (callers get
/// `WindowClosed`) and answers queued requests with `NoBackendAvailable` (later ones fail to send
/// and are answered the same way).
pub(crate) struct Runtime {
    /// Concrete backend (never `Auto`): an egui theme, or `Win32` = TaskDialog routing.
    backend: XDialogBackend,
    /// A failing egui dialog falls back to TaskDialogs (Windows `Auto`).
    #[cfg(windows)]
    fallback: bool,
    /// Created on first use (`Win32` routing or fallback).
    #[cfg(windows)]
    win32: Option<TaskDialogManager>,
    /// Light/dark override, re-resolved on appearance changes.
    xtheme: XDialogTheme,
    inbox: Arc<Inbox>,
    rx: Receiver<DialogMessageRequest>,
    dialogs: BTreeMap<usize, DialogWindow>,
    /// Windows of closed dialogs, until their `Destroyed` arrives.
    retired: Vec<WindowId>,
    /// Created with the first window.
    sb: Option<softbuffer::Context<OwnedDisplayHandle>>,
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
            return Err(XDialogError::SystemError("xdialog: a dialog backend is already initialized".into()));
        }
        FontRegistry::global().start_background_scan();
        Ok(Runtime { backend,
                     #[cfg(windows)]
                     fallback,
                     #[cfg(windows)]
                     win32: None,
                     xtheme,
                     inbox,
                     rx,
                     dialogs: BTreeMap::new(),
                     retired: Vec::new(),
                     sb: None,
                     exit: false,
                     _ui_thread: UiThreadMark::set() })
    }

    /// Handle queued requests, refresh after font/appearance changes, request due redraws.
    /// Returns the next time an iteration is needed (`None` = idle).
    pub(crate) fn about_to_wait(&mut self, el: &ActiveEventLoop) -> Option<Instant> {
        // Cleared before draining: a request sent from now on wakes the loop again.
        self.inbox.begin_drain();
        let mut refresh = false;
        while let Ok(msg) = self.rx.try_recv() {
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

        let now = Instant::now();
        let mut next: Option<Instant> = None;
        for w in self.dialogs.values_mut() {
            let s = w.dialog.schedule_mut();
            if s.due(now) {
                s.fired();
                w.window.request_redraw();
            } else if let Some(t) = s.next {
                next = Some(next.map_or(t, |n| n.min(t)));
            }
        }
        next
    }

    /// Handle a window event. `true` if `id` is (or was, until its `Destroyed`) a dialog window.
    pub(crate) fn window_event(&mut self, id: WindowId, ev: &WindowEvent) -> bool {
        if self.retired.contains(&id) {
            if matches!(ev, WindowEvent::Destroyed) {
                self.retired.retain(|r| *r != id);
            }
            return true;
        }
        let Some(key) = self.dialogs.iter().find(|(_, w)| w.window.id() == id).map(|(k, _)| *k) else { return false };
        self.guarded(Some(key), "handling a window event", |rt| rt.dialog_event(key, ev));
        true
    }

    /// Every dialog window (open, or closed and not yet Destroyed).
    #[cfg(feature = "winit-host")]
    pub(crate) fn window_ids(&self) -> Vec<WindowId> {
        self.dialogs.values().map(|w| w.window.id()).chain(self.retired.iter().copied()).collect()
    }

    fn dialog_event(&mut self, key: usize, ev: &WindowEvent) {
        let Some(w) = self.dialogs.get_mut(&key) else { return };
        match ev {
            WindowEvent::RedrawRequested => w.redraw(),
            WindowEvent::Resized(s) => w.dialog.resized([s.width, s.height]),
            // winit keeps the logical size itself; the following `Resized` has the new size.
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => w.dialog.scale_changed(*scale_factor as f32),
            WindowEvent::CloseRequested => w.dialog.finish(XDialogResult::WindowClosed),
            WindowEvent::ThemeChanged(_) => w.dialog.refresh_appearance(),
            // Never reach the dialog: synthetic keys (focus changes), file drops (would pile up in
            // egui-winit's input), pan gestures (egui-winit scrolls with them), non-primary buttons.
            WindowEvent::KeyboardInput { is_synthetic: true, .. }
            | WindowEvent::HoveredFile(_)
            | WindowEvent::HoveredFileCancelled
            | WindowEvent::DroppedFile(_)
            | WindowEvent::PanGesture { .. } => {}
            WindowEvent::MouseInput { button, .. } if *button != MouseButton::Left => {}
            _ => {
                // egui-winit drops a release while the pointer is outside the window: cancel the
                // press instead (no activation, no stuck pressed look).
                if matches!(ev, WindowEvent::MouseInput { state: ElementState::Released, .. }) && !w.input.is_pointer_in_window() {
                    w.dialog.cancel_press();
                }
                let _ = w.input.on_window_event(&w.window, ev);
                let mut events = std::mem::take(&mut w.input.egui_input_mut().events);
                // egui-winit always reports `repeat: false`; winit knows the OS auto-repeat.
                if let WindowEvent::KeyboardInput { event, .. } = ev {
                    for e in &mut events {
                        if let egui::Event::Key { repeat, .. } = e {
                            *repeat = event.repeat;
                        }
                    }
                }
                if !events.is_empty() {
                    w.dialog.handle_events(events);
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
            DialogMessageRequest::ShowMessageWindow(id, options, creation) => self.show(el, id, DialogKind::Message, options, creation, None),
            DialogMessageRequest::ShowProgressWindow(id, options, creation, callback) => {
                self.show(el, id, DialogKind::Progress, options, creation, callback)
            }
            DialogMessageRequest::CloseWindow(id)
            | DialogMessageRequest::SetProgressIndeterminate(id)
            | DialogMessageRequest::SetProgressValue(id, _)
            | DialogMessageRequest::SetProgressText(id, _) => {
                let Some(w) = self.dialogs.get_mut(&id) else {
                    // Not an egui dialog: a TaskDialog (Win32 routing / fallback), if any.
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
            creation: CreationSender,
            mut callback: Option<ProgressButtonCallback>) {
        #[cfg(windows)]
        if self.backend == XDialogBackend::Win32 {
            self.win32().show(id, options, kind == DialogKind::Progress, creation, callback);
            return;
        }
        if self.dialogs.contains_key(&id) {
            let _ = creation.send(Err(XDialogError::SystemError(format!("xdialog: dialog id {id} already exists"))));
            return;
        }
        // A panic while building the dialog or in its first frame is a failure like any other:
        // the request is still intact here.
        let shown = catch_unwind(AssertUnwindSafe(|| self.try_show(el, id, kind, &options, &mut callback)))
            .unwrap_or_else(|_| Err(XDialogError::SystemError("xdialog: building the dialog panicked".into())));
        match shown {
            Ok(result) => {
                let _ = creation.send(Ok(result));
                self.after(id);
            }
            Err(e) => {
                #[cfg(windows)]
                if self.fallback {
                    warn!("xdialog: {e}; using Win32 TaskDialog for the rest of the session");
                    self.backend = XDialogBackend::Win32;
                    self.win32().show(id, options, kind == DialogKind::Progress, creation, callback);
                    return;
                }
                error!("xdialog: could not show dialog {id}: {e}");
                let _ = creation.send(Err(e));
            }
        }
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
                -> Result<mpsc::Receiver<XDialogResult>, XDialogError> {
        let primary = el.primary_monitor().or_else(|| el.available_monitors().next());
        let ppp = primary.as_ref().map_or(1.0, |m| m.scale_factor() as f32);
        let max_height = primary.as_ref()
                                .map(|m| (m.size().height as f64 / m.scale_factor() * 0.9) as f32)
                                .filter(|h| h.is_finite() && *h > 0.0)
                                .unwrap_or(DEFAULT_MAX_HEIGHT);
        let (tx, result) = mpsc::channel();
        let params = DialogParams { id,
                                    content: DialogContent::new(kind, options.clone()),
                                    appearance: resolve_appearance(self.xtheme),
                                    system_appearance: Some(self.xtheme),
                                    ppp,
                                    max_height,
                                    clock: DialogClock::new(),
                                    sender: Some(tx) };
        let mut dialog = Dialog::new(theme::new(self.backend), params);

        let size = dialog.desired_size();
        let dark = dialog.dark_titlebar();
        // On Windows winit only reports `ThemeChanged` for windows created WITHOUT a preferred
        // theme, so a dialog that follows the system gets `None` there (the title bar is set
        // through DWM right after creation and on every appearance change).
        let follow_system = cfg!(windows) && self.xtheme == XDialogTheme::SystemDefault;
        let mut attrs = Window::default_attributes().with_title(dialog.title())
                                                    .with_inner_size(LogicalSize::new(size.x as f64, size.y as f64))
                                                    .with_resizable(false)
                                                    .with_enabled_buttons(WindowButtons::CLOSE)
                                                    .with_visible(false)
                                                    .with_active(!no_activate())
                                                    .with_theme((!follow_system).then_some(winit_theme(dark)));
        #[cfg(windows)]
        {
            // Rounded corners (Windows 11). No drag and drop: winit's default registers an OLE drop
            // target, which needs (and on an uninitialised host thread silently makes) an STA
            // thread and aborts on an MTA one. Dialogs take no drops.
            use winit::platform::windows::{CornerPreference, WindowAttributesExtWindows};
            attrs = attrs.with_drag_and_drop(false).with_corner_preference(CornerPreference::Round);
        }
        let px = dialog.physical_size(ppp);
        let virtual_left = || el.available_monitors().map(|m| m.position().x).min().unwrap_or(0);
        let position = test_position(px, virtual_left).or_else(|| {
                           // Centred on the primary monitor (physical), computed up front so the
                           // window manager doesn't reposition it after mapping.
                           let m = primary.as_ref()?;
                           let (msize, mpos) = (m.size(), m.position());
                           Some([mpos.x + (msize.width as i32 - px[0] as i32) / 2, mpos.y + (msize.height as i32 - px[1] as i32) / 2])
                       });
        if let Some([x, y]) = position {
            attrs = attrs.with_position(PhysicalPosition::new(x, y));
        }
        let window = Rc::new(el.create_window(attrs).map_err(|e| XDialogError::SystemError(format!("xdialog: could not create a window: {e}")))?);
        // Until the dialog is complete: a failure (or panic) below drops the window, and its late
        // `Destroyed` must still be recognised as xdialog's.
        self.retired.push(window.id());
        #[cfg(windows)]
        super::platform_win::set_dark_titlebar(&window, dark);

        let presenter = self.presenter(el, &window)?;
        let scale = window.scale_factor() as f32;
        let input = egui_winit::State::new(dialog.ctx().clone(), ViewportId::ROOT, el, Some(scale), None, Some(MAX_TEXTURE_SIDE));
        let s = window.inner_size();
        dialog.attach(Box::new(presenter), scale, [s.width, s.height]);
        window.set_visible(true);
        // Present right away: presenting to a hidden window is a no-op on Win32/X11 and the class
        // background would flash.
        dialog.frame().map_err(|e| XDialogError::SystemError(format!("xdialog: could not present: {e}")))?;
        dialog.set_callback(callback.take());
        self.retired.retain(|r| *r != window.id());
        self.dialogs.insert(id, DialogWindow { dialog, window, input, period: None });
        Ok(result)
    }

    /// A surface for `window` (the shared softbuffer context is created with the first window).
    fn presenter(&mut self, el: &ActiveEventLoop, window: &Rc<Window>) -> Result<SoftwarePresenter, XDialogError> {
        let err = |e: &dyn std::fmt::Display| XDialogError::SystemError(format!("xdialog: could not create a surface: {e}"));
        let sb = match &mut self.sb {
            Some(sb) => sb,
            empty => empty.insert(softbuffer::Context::new(el.owned_display_handle()).map_err(|e| err(&e))?),
        };
        SoftwarePresenter::new(sb, window.clone()).map_err(|e| err(&e))
    }

    /// Hide `window` now and remember its id until winit reports it destroyed (the caller drops it).
    fn retire(&mut self, window: &Window) {
        window.set_visible(false);
        self.retired.push(window.id());
    }

    #[cfg(windows)]
    fn win32(&mut self) -> &TaskDialogManager {
        self.win32.get_or_insert_with(TaskDialogManager::new)
    }

    /// Post-processing after anything touched dialog `key`: remove a closed dialog (hide, drop
    /// presenter and window), forward resize / title-bar requests.
    fn after(&mut self, key: usize) {
        let Some(w) = self.dialogs.get_mut(&key) else { return };
        if w.dialog.is_closed() {
            let w = self.dialogs.remove(&key).expect("present");
            self.retire(&w.window);
            return;
        }
        if let Some(size) = w.dialog.take_resize() {
            // `Some`: applied at once, and winit may not emit `Resized` for it (Wayland).
            if let Some(s) = w.window.request_inner_size(LogicalSize::new(size.x as f64, size.y as f64)) {
                w.dialog.resized([s.width, s.height]);
            }
        }
        if let Some(dark) = w.dialog.take_titlebar_change() {
            // Windows: DWM directly (`Window::set_theme` doesn't change winit's preferred theme,
            // and `ThemeChanged` must keep flowing for system-following dialogs).
            #[cfg(windows)]
            super::platform_win::set_dark_titlebar(&w.window, dark);
            #[cfg(not(windows))]
            w.window.set_theme(Some(winit_theme(dark)));
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

    /// Close every dialog, egui (`WindowClosed`, windows hidden and dropped) and TaskDialog.
    fn close_all(&mut self) {
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
        self.rx.try_iter().for_each(crate::channel::reject);
    }
}

impl DialogWindow {
    /// Render a frame (`RedrawRequested`).
    fn redraw(&mut self) {
        let period = self.monitor_period();
        self.dialog.schedule_mut().set_monitor_period(period);
        if let Err(e) = self.dialog.frame() {
            warn!("xdialog: present failed: {e}");
        }
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
#[cfg(target_os = "linux")]
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

fn winit_theme(dark: bool) -> winit::window::Theme {
    if dark {
        winit::window::Theme::Dark
    } else {
        winit::window::Theme::Light
    }
}

/// `XDIALOG_TEST_NO_ACTIVATE=1`: never activate/focus dialog windows. Honoured in all builds.
fn no_activate() -> bool {
    std::env::var_os("XDIALOG_TEST_NO_ACTIVATE").is_some_and(|v| !v.is_empty() && v != "0")
}

/// `XDIALOG_TEST_POS=x,y|offscreen` (test builds only): physical top-left of a new window of
/// `size_px`; `virtual_left` is the left edge of the whole virtual desktop (physical px).
fn test_position(size_px: [u32; 2], virtual_left: impl FnOnce() -> i32) -> Option<[i32; 2]> {
    if !test_env_enabled() {
        return None;
    }
    let v = std::env::var("XDIALOG_TEST_POS").ok()?;
    let v = v.trim();
    if v.eq_ignore_ascii_case("offscreen") {
        // Left of the whole virtual desktop: never on a real monitor.
        return Some([virtual_left() - size_px[0] as i32 - 64, 64]);
    }
    let (x, y) = v.split_once(',')?;
    Some([x.trim().parse().ok()?, y.trim().parse().ok()?])
}

// -------------------------------------------------------------------------------------------------
// Test hooks (`XDialogApp::test_*`)
// -------------------------------------------------------------------------------------------------

#[cfg(all(feature = "winit-host", feature = "_test-hooks"))]
impl Runtime {
    pub(crate) fn test_dialogs(&self) -> Vec<crate::host::LiveDialog> {
        self.dialogs
            .iter()
            .map(|(&id, w)| crate::host::LiveDialog { id,
                                                      title: w.dialog.title().to_owned(),
                                                      button_rects: w.dialog.button_rects(),
                                                      frames: w.dialog.frames() })
            .collect()
    }

    pub(crate) fn test_inject(&mut self, id: usize, ev: egui::Event) {
        if let Some(w) = self.dialogs.get_mut(&id) {
            w.dialog.handle_events([ev]);
            self.after(id);
        }
    }
}
