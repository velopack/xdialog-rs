//! Dialog manager: request dispatch, window creation sequence, event and
//! redraw routing, repaint deadlines, closing. Generic over the theme and the window system, so the
//! own winit loop (`own_loop.rs`) and host mode (`host.rs`) share it.
//!
//! Window creation:
//! 1. `Dialog::new` builds the egui context (fonts before any pass) and runs the measure pass at
//!    the window system's expected scale.
//! 2. `ws.create` makes an INVISIBLE window at exactly `desired_size` (logical). On failure the
//!    creation request gets `Err`.
//! 3. The presenter is attached (a different real scale re-requests the size).
//! 4. `set_visible(true)`, then the first frame is rendered and presented immediately.
//! 5. `creation.send(Ok(result_rx))`.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Instant;

use super::appearance::{resolve_appearance, watch};
use super::clock::DialogClock;
use super::dialog::{AppearanceSource, Dialog, DialogContent, DialogParams, ResultSink};
use super::fonts::{FontRegistry, Waker};
use super::testhooks::{no_activate, test_env_enabled};
use super::theme::{DialogKind, SizeLimits, Theme};
use super::window_system::{WindowSpec, WindowSystem};
use crate::backends::host_types::HostEvent;
use crate::model::{CreationSender, DialogMessageRequest, XDialogOptions, XDialogResult, XDialogTheme};
use crate::{ProgressButtonCallback, XDialogError};

/// Height cap when the window system doesn't know the monitor (host mode), logical px.
const DEFAULT_MAX_HEIGHT: f32 = 800.0;

struct Entry<T: Theme, W> {
    dialog: Dialog<T>,
    win: W,
}

/// All open dialogs of one loop (own loop or host thread).
pub(crate) struct Manager<T: Theme, W> {
    theme: T,
    xtheme: XDialogTheme,
    dialogs: BTreeMap<usize, Entry<T, W>>,
    font_generation: u64,
    #[cfg(xd_test_hooks)]
    remote: Option<live::Remote>,
}

impl<T: Theme, W> Manager<T, W> {
    /// `waker` (optional) is called from background threads when fonts or the appearance changed;
    /// the loop then calls [`Manager::refresh`].
    pub(crate) fn new(theme: T, xtheme: XDialogTheme, waker: Option<Waker>) -> Self {
        let reg = FontRegistry::global();
        reg.start_background_scan();
        if let Some(w) = waker {
            reg.add_waker(w.clone());
            watch(w);
        }
        Manager { theme,
                  xtheme,
                  dialogs: BTreeMap::new(),
                  font_generation: reg.generation(),
                  #[cfg(xd_test_hooks)]
                  remote: None }
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.dialogs.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.dialogs.len()
    }

    /// The dialog whose window satisfies `pred` (few dialogs: a linear scan).
    pub(crate) fn dialog_for(&self, pred: impl Fn(&W) -> bool) -> Option<usize> {
        self.dialogs.iter().find(|(_, e)| pred(&e.win)).map(|(id, _)| *id)
    }

    #[cfg(test)]
    pub(crate) fn dialog(&self, id: usize) -> Option<&Dialog<T>> {
        self.dialogs.get(&id).map(|e| &e.dialog)
    }

    /// Physical client size dialog `id` wants at scale `ppp` (own loop: DPI change).
    pub(crate) fn desired_physical_size(&self, id: usize, ppp: f32) -> Option<[u32; 2]> {
        self.dialogs.get(&id).map(|e| e.dialog.physical_size(ppp))
    }

    // ---------------------------------------------------------------------------------------------
    // Requests
    // ---------------------------------------------------------------------------------------------

    /// Handle one request. Returns `false` for `ExitEventLoop` (every dialog was closed; the
    /// caller exits its loop), `true` otherwise.
    pub(crate) fn handle_request<WS: WindowSystem<Win = W>>(&mut self, ws: &mut WS, req: DialogMessageRequest) -> bool {
        match req {
            DialogMessageRequest::None => {}
            DialogMessageRequest::ExitEventLoop => {
                self.close_all(ws);
                return false;
            }
            DialogMessageRequest::CloseWindow(id) => {
                if let Some(e) = self.dialogs.get_mut(&id) {
                    e.dialog.finish(XDialogResult::WindowClosed);
                    self.after(ws, id);
                }
            }
            DialogMessageRequest::ShowMessageWindow(id, options, creation) => {
                self.create(ws, id, DialogKind::Message, options, creation, None);
            }
            DialogMessageRequest::ShowProgressWindow(id, options, creation, callback) => {
                self.create(ws, id, DialogKind::Progress, options, creation, callback);
            }
            DialogMessageRequest::SetProgressIndeterminate(id) => {
                if let Some(e) = self.dialogs.get_mut(&id) {
                    e.dialog.set_progress_indeterminate();
                }
            }
            DialogMessageRequest::SetProgressValue(id, value) => {
                if let Some(e) = self.dialogs.get_mut(&id) {
                    e.dialog.set_progress_value(value);
                }
            }
            DialogMessageRequest::SetProgressText(id, text) => {
                if let Some(e) = self.dialogs.get_mut(&id) {
                    e.dialog.set_text(&self.theme, &text);
                    self.after(ws, id);
                }
            }
        }
        true
    }

    fn create<WS: WindowSystem<Win = W>>(&mut self,
                                          ws: &mut WS,
                                          id: usize,
                                          kind: DialogKind,
                                          options: XDialogOptions,
                                          creation: CreationSender,
                                          callback: Option<ProgressButtonCallback>) {
        if self.dialogs.contains_key(&id) {
            let _ = creation.send(Err(XDialogError::SystemError(format!("xdialog: dialog id {id} already exists"))));
            return;
        }
        let (tx, rx) = oneshot::channel();
        let theme = &self.theme;
        let xtheme = self.xtheme.clone();
        let ppp = ws.expected_ppp();
        let limits = SizeLimits { max_height: ws.max_client_height().filter(|h| h.is_finite() && *h > 0.0).unwrap_or(DEFAULT_MAX_HEIGHT) };
        let built = catch_unwind(AssertUnwindSafe(|| {
                                     let params = DialogParams { id,
                                                                 content: DialogContent::new(kind, options),
                                                                 appearance: resolve_appearance(&xtheme),
                                                                 appearance_source: AppearanceSource::System(xtheme.clone()),
                                                                 ppp,
                                                                 limits,
                                                                 clock: DialogClock::new(),
                                                                 sink: ResultSink { sender: Some(tx), callback } };
                                     Dialog::new(theme, params)
                                 }));
        let mut dialog = match built {
            Ok(d) => d,
            Err(_) => {
                error!("xdialog: building dialog {id} panicked");
                let _ = creation.send(Err(XDialogError::SystemError("xdialog: building the dialog panicked".into())));
                return;
            }
        };

        let size = dialog.desired_size();
        let spec = WindowSpec { title: dialog.title(),
                                inner_size: size,
                                dark_titlebar: dialog.dark_titlebar(),
                                follow_system: self.xtheme == XDialogTheme::SystemDefault,
                                active: !no_activate(),
                                position: test_position(ws, dialog.physical_size(ppp)) };
        let created = match ws.create(&spec) {
            Ok(c) => c,
            Err(e) => {
                error!("xdialog: could not create a window for dialog {id}: {e}");
                // Nothing was delivered yet: dropping the dialog must not send WindowClosed first.
                dialog.finish(XDialogResult::WindowClosed);
                let _ = creation.send(Err(e));
                return;
            }
        };
        dialog.attach(created.presenter, created.ppp, created.size_px);
        // Registered before anything else can panic, so the window is always owned by an entry.
        self.dialogs.insert(id, Entry { dialog, win: created.win });
        let shown = catch_unwind(AssertUnwindSafe(|| {
                                     let e = self.dialogs.get_mut(&id).expect("just inserted");
                                     ws.set_visible(&e.win, true);
                                     // Present right away: presenting to a hidden window is a no-op on
                                     // Win32/X11 and would flash the class background.
                                     e.dialog.frame(&self.theme);
                                 }));
        if shown.is_err() {
            error!("xdialog: showing dialog {id} panicked");
            // Nothing was delivered yet: remove the half-created dialog and its window here, so
            // neither a leftover window nor a `WindowClosed` reaches anyone.
            let cleanup = catch_unwind(AssertUnwindSafe(|| {
                                           if let Some(Entry { mut dialog, win }) = self.dialogs.remove(&id) {
                                               dialog.finish(XDialogResult::WindowClosed);
                                               ws.set_visible(&win, false);
                                               drop(dialog.detach());
                                               ws.destroy(win);
                                           }
                                       }));
            if cleanup.is_err() {
                error!("xdialog: cleaning up dialog {id} after a panic panicked again");
            }
            let _ = creation.send(Err(XDialogError::SystemError("xdialog: showing the dialog panicked".into())));
            return;
        }
        #[cfg(xd_test_hooks)]
        self.publish(ws, id);
        let _ = creation.send(Ok(rx));
        self.after(ws, id);
    }

    // ---------------------------------------------------------------------------------------------
    // Window events and frames
    // ---------------------------------------------------------------------------------------------

    /// Forward one window event to dialog `id`.
    pub(crate) fn handle_event<WS: WindowSystem<Win = W>>(&mut self, ws: &mut WS, id: usize, ev: HostEvent) {
        let Some(e) = self.dialogs.get_mut(&id) else { return };
        e.dialog.handle_event(&self.theme, ev);
        self.after(ws, id);
    }

    /// Render dialog `id` (window `RedrawRequested`).
    pub(crate) fn redraw<WS: WindowSystem<Win = W>>(&mut self, ws: &mut WS, id: usize) {
        let Some(e) = self.dialogs.get_mut(&id) else { return };
        e.dialog.schedule_mut().set_monitor_period(ws.monitor_period(&e.win));
        e.dialog.frame(&self.theme);
        if e.dialog.take_present_failed() {
            if let Some(p) = ws.recreate_presenter(&e.win) {
                e.dialog.replace_presenter(&self.theme, p);
            }
        }
        #[cfg(xd_test_hooks)]
        self.publish(ws, id);
        self.after(ws, id);
    }

    /// Request redraws for due dialogs; returns the earliest future deadline.
    pub(crate) fn poll_timers<WS: WindowSystem<Win = W>>(&mut self, ws: &mut WS, now: Instant) -> Option<Instant> {
        let mut next: Option<Instant> = None;
        for e in self.dialogs.values_mut() {
            let s = e.dialog.schedule_mut();
            if s.due(now) {
                s.fired();
                ws.request_redraw(&e.win);
            } else if let Some(t) = s.next {
                next = Some(next.map_or(t, |n| n.min(t)));
            }
        }
        next
    }

    /// Fonts or appearance may have changed (a background thread woke the loop).
    pub(crate) fn refresh<WS: WindowSystem<Win = W>>(&mut self, ws: &mut WS) {
        let gen = FontRegistry::global().generation();
        let fonts = std::mem::replace(&mut self.font_generation, gen) != gen;
        let ids: Vec<usize> = self.dialogs.keys().copied().collect();
        for id in ids {
            if let Some(e) = self.dialogs.get_mut(&id) {
                if fonts {
                    e.dialog.refresh_fonts(&self.theme);
                }
                e.dialog.refresh_appearance(&self.theme);
            }
            self.after(ws, id);
        }
    }

    /// Close every dialog: `WindowClosed` to waiters, presenters dropped, windows destroyed.
    pub(crate) fn close_all<WS: WindowSystem<Win = W>>(&mut self, ws: &mut WS) {
        let ids: Vec<usize> = self.dialogs.keys().copied().collect();
        for id in ids {
            if let Some(e) = self.dialogs.get_mut(&id) {
                e.dialog.finish(XDialogResult::WindowClosed);
            }
            self.after(ws, id);
        }
    }

    /// Post-processing after anything touched dialog `id`: remove a closed dialog (hide, drop the
    /// presenter, destroy the window), forward resize / title-bar requests.
    fn after<WS: WindowSystem<Win = W>>(&mut self, ws: &mut WS, id: usize) {
        let Some(e) = self.dialogs.get_mut(&id) else { return };
        if e.dialog.is_closed() {
            let Entry { mut dialog, win } = self.dialogs.remove(&id).expect("present");
            ws.set_visible(&win, false);
            drop(dialog.detach());
            ws.destroy(win);
            drop(dialog);
            #[cfg(xd_test_hooks)]
            live::remove(id);
            return;
        }
        if let Some(size) = e.dialog.take_resize() {
            if let Some([width, height]) = ws.request_inner_size(&e.win, size) {
                // Applied at once without a resize event: take the new size (schedules a frame).
                e.dialog.handle_event(&self.theme, HostEvent::Resized { width, height });
            }
        }
        if let Some(dark) = e.dialog.take_titlebar_change() {
            ws.set_dark_titlebar(&e.win, dark);
        }
    }
}

/// The dialog a request concerns (closed after a panic while handling it), if any.
pub(crate) fn request_id(msg: &DialogMessageRequest) -> Option<usize> {
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

/// `XDIALOG_TEST_POS=x,y|offscreen` (test builds only): physical top-left.
fn test_position<WS: WindowSystem>(ws: &WS, size_px: [u32; 2]) -> Option<[i32; 2]> {
    if !test_env_enabled() {
        return None;
    }
    let v = std::env::var("XDIALOG_TEST_POS").ok()?;
    let v = v.trim();
    if v.eq_ignore_ascii_case("offscreen") {
        // Left of the whole virtual desktop: never on a real monitor.
        return Some([ws.virtual_screen_left() - size_px[0] as i32 - 64, 64]);
    }
    let (x, y) = v.split_once(',')?;
    Some([x.trim().parse().ok()?, y.trim().parse().ok()?])
}

// -------------------------------------------------------------------------------------------------
// Test hooks: live-dialog registry (`xdialog::__test::{live_dialogs, inject}`)
// -------------------------------------------------------------------------------------------------

#[cfg(xd_test_hooks)]
impl<T: Theme, W> Manager<T, W> {
    /// Route test-hook commands for this manager's dialogs through `remote` (the loop forwards
    /// them back to [`Manager::handle_remote`] on its own thread).
    pub(crate) fn set_remote(&mut self, remote: live::Remote) {
        self.remote = Some(remote);
    }

    /// Apply a test-hook command on the loop thread.
    pub(crate) fn handle_remote<WS: WindowSystem<Win = W>>(&mut self, ws: &mut WS, cmd: live::RemoteCmd) {
        let live::RemoteCmd::Inject(id, ev) = cmd;
        self.handle_event(ws, id, ev);
    }

    fn publish<WS: WindowSystem<Win = W>>(&self, ws: &WS, id: usize) {
        let (Some(e), Some(remote)) = (self.dialogs.get(&id), self.remote.as_ref()) else { return };
        let d = &e.dialog;
        let ppp = d.ppp();
        let info = live::LiveDialog { id,
                                    title: d.title().to_owned(),
                                    raw_window: ws.raw_window_id(&e.win),
                                    size_px: (d.size_px()[0], d.size_px()[1]),
                                    ppp,
                                    button_rects_px: d.button_rects().into_iter().map(|r| r.map(|v| v * ppp)).collect(),
                                    frames: d.frames() };
        live::publish(info, remote.clone());
    }
}

#[cfg(xd_test_hooks)]
pub(crate) mod live {
    //! Process-wide registry of live dialogs (own loop and host mode), for the hidden `__test`
    //! API. Commands are routed to the owning loop through its [`Remote`].

    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex, OnceLock};

    use crate::backends::host_types::HostEvent;

    /// A command for a live dialog, executed on its loop thread.
    #[derive(Clone, Debug)]
    pub(crate) enum RemoteCmd {
        Inject(usize, HostEvent),
    }

    impl RemoteCmd {
        /// The dialog the command is for.
        pub(crate) fn id(&self) -> usize {
            let RemoteCmd::Inject(id, _) = self;
            *id
        }
    }

    /// Delivers a command to the owning loop (wakes it).
    pub(crate) type Remote = Arc<dyn Fn(RemoteCmd) + Send + Sync>;

    pub(crate) use crate::backends::egui_core::testhooks::api::LiveDialog;

    fn reg() -> &'static Mutex<BTreeMap<usize, (LiveDialog, Remote)>> {
        static R: OnceLock<Mutex<BTreeMap<usize, (LiveDialog, Remote)>>> = OnceLock::new();
        R.get_or_init(|| Mutex::new(BTreeMap::new()))
    }

    pub(super) fn publish(info: LiveDialog, remote: Remote) {
        reg().lock().unwrap_or_else(|e| e.into_inner()).insert(info.id, (info, remote));
    }

    pub(super) fn remove(id: usize) {
        reg().lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    }

    /// All live dialogs.
    pub(crate) fn snapshot() -> Vec<LiveDialog> {
        reg().lock().unwrap_or_else(|e| e.into_inner()).values().map(|(i, _)| i.clone()).collect()
    }

    /// Send a command to the loop owning dialog `id`. `false` if no such live dialog.
    pub(crate) fn send(cmd: RemoteCmd) -> bool {
        let id = cmd.id();
        let remote = reg().lock().unwrap_or_else(|e| e.into_inner()).get(&id).map(|(_, r)| r.clone());
        match remote {
            Some(r) => {
                r(cmd);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::egui_core::dialog::tests::StubTheme;
    use crate::backends::egui_core::render::{MemoryPresenter, Presenter};
    use crate::backends::egui_core::window_system::CreatedWindow;
    use crate::backends::host_types::{Key, MouseButton};
    use egui::Vec2;

    /// A window system that records calls and uses memory presenters.
    #[derive(Default)]
    struct MockWs {
        next: u32,
        log: Vec<String>,
        fail: bool,
        ppp: f32,
        panic_on_show: bool,
    }

    impl WindowSystem for MockWs {
        type Win = u32;
        fn create(&mut self, spec: &WindowSpec<'_>) -> Result<CreatedWindow<u32>, XDialogError> {
            if self.fail {
                return Err(XDialogError::SystemError("no windows".into()));
            }
            self.next += 1;
            let ppp = if self.ppp > 0.0 { self.ppp } else { 1.0 };
            let s = spec.inner_size * ppp;
            self.log.push(format!("create {} {}x{} active={}", self.next, spec.inner_size.x, spec.inner_size.y, spec.active));
            Ok(CreatedWindow { win: self.next, presenter: Box::new(MemoryPresenter::new()) as Box<dyn Presenter>, ppp, size_px: [s.x.round() as u32, s.y.round() as u32] })
        }
        fn set_visible(&mut self, w: &u32, visible: bool) {
            self.log.push(format!("visible {w} {visible}"));
            if visible && self.panic_on_show {
                panic!("set_visible failed");
            }
        }
        fn request_inner_size(&mut self, w: &u32, logical: Vec2) -> Option<[u32; 2]> {
            self.log.push(format!("size {w} {}x{}", logical.x, logical.y));
            None
        }
        fn request_redraw(&mut self, w: &u32) {
            self.log.push(format!("redraw {w}"));
        }
        fn destroy(&mut self, w: u32) {
            self.log.push(format!("destroy {w}"));
        }
        fn max_client_height(&self) -> Option<f32> {
            None
        }
    }

    fn opts(buttons: &[&str]) -> XDialogOptions {
        XDialogOptions { title: "T".into(),
                         main_instruction: "H".into(),
                         message: "Body".into(),
                         icon: crate::model::XDialogIcon::None,
                         buttons: buttons.iter().map(|s| s.to_string()).collect() }
    }

    fn show(m: &mut Manager<StubTheme, u32>, ws: &mut MockWs, id: usize, buttons: &[&str]) -> oneshot::Receiver<XDialogResult> {
        let (tx, rx) = oneshot::channel();
        assert!(m.handle_request(ws, DialogMessageRequest::ShowMessageWindow(id, opts(buttons), tx)));
        rx.recv().unwrap().unwrap()
    }

    #[test]
    fn create_show_and_close_sequence() {
        let mut m = Manager::new(StubTheme::linux(), XDialogTheme::Light, None);
        let mut ws = MockWs::default();
        let rx = show(&mut m, &mut ws, 1, &["OK"]);
        assert!(ws.log[0].starts_with("create 1 220x"), "{:?}", ws.log);
        assert_eq!(ws.log[1], "visible 1 true");
        assert_eq!(m.dialog(1).unwrap().frames(), 1, "first frame presented before the ack");
        assert!(m.handle_request(&mut ws, DialogMessageRequest::CloseWindow(1)));
        assert_eq!(rx.recv().unwrap(), XDialogResult::WindowClosed);
        assert_eq!(&ws.log[ws.log.len() - 2..], ["visible 1 false", "destroy 1"]);
        assert!(m.is_empty());
    }

    #[test]
    fn creation_failure_reports_error() {
        let mut m = Manager::new(StubTheme::linux(), XDialogTheme::Light, None);
        let mut ws = MockWs { fail: true, ..Default::default() };
        let (tx, rx) = oneshot::channel();
        m.handle_request(&mut ws, DialogMessageRequest::ShowMessageWindow(1, opts(&["OK"]), tx));
        assert!(matches!(rx.recv().unwrap(), Err(XDialogError::SystemError(_))));
        assert!(m.is_empty());
    }

    #[test]
    fn panic_while_showing_destroys_the_window() {
        let mut m = Manager::new(StubTheme::linux(), XDialogTheme::Light, None);
        let mut ws = MockWs { panic_on_show: true, ..Default::default() };
        let (tx, rx) = oneshot::channel();
        assert!(m.handle_request(&mut ws, DialogMessageRequest::ShowMessageWindow(1, opts(&["OK"]), tx)));
        assert!(matches!(rx.recv().unwrap(), Err(XDialogError::SystemError(_))));
        assert!(m.is_empty());
        assert_eq!(&ws.log[ws.log.len() - 2..], ["visible 1 false", "destroy 1"]);
    }

    #[test]
    fn events_route_and_timers_fire() {
        let mut m = Manager::new(StubTheme::linux(), XDialogTheme::Light, None);
        let mut ws = MockWs::default();
        let rx = show(&mut m, &mut ws, 3, &["A", "B"]);
        let id = m.dialog_for(|w| *w == 1).unwrap();
        assert_eq!(id, 3);
        m.handle_event(&mut ws, id, HostEvent::CursorMoved { x: 5.0, y: 5.0 });
        // Input asks for an immediate frame.
        let _ = m.poll_timers(&mut ws, Instant::now() + std::time::Duration::from_millis(20));
        assert_eq!(ws.log.last().unwrap(), "redraw 1");
        m.redraw(&mut ws, id);
        m.handle_event(&mut ws, id, HostEvent::Key { key: Key::Enter, pressed: true, repeat: false });
        assert_eq!(rx.recv().unwrap(), XDialogResult::ButtonPressed(1));
        assert!(m.is_empty());
        // Unknown ids are ignored.
        m.handle_event(&mut ws, 99, HostEvent::MouseButton { button: MouseButton::Primary, pressed: true });
        m.redraw(&mut ws, 99);
    }

    #[test]
    fn exit_closes_everything_and_text_resizes() {
        let mut m = Manager::new(StubTheme::linux(), XDialogTheme::Light, None);
        let mut ws = MockWs::default();
        let (tx, rx) = oneshot::channel();
        m.handle_request(&mut ws, DialogMessageRequest::ShowProgressWindow(5, opts(&[]), tx, None));
        let rx = rx.recv().unwrap().unwrap();
        m.handle_request(&mut ws, DialogMessageRequest::SetProgressText(5, "long words ".repeat(50)));
        m.redraw(&mut ws, 5);
        assert!(ws.log.iter().any(|l| l.starts_with("size 1 ")), "{:?}", ws.log);
        let rx2 = show(&mut m, &mut ws, 6, &["OK"]);
        assert_eq!(m.len(), 2);
        assert!(!m.handle_request(&mut ws, DialogMessageRequest::ExitEventLoop));
        assert!(m.is_empty());
        assert_eq!(rx.recv().unwrap(), XDialogResult::WindowClosed);
        assert_eq!(rx2.recv().unwrap(), XDialogResult::WindowClosed);
    }

    #[test]
    fn other_scale_than_expected_requests_size() {
        let mut m = Manager::new(StubTheme::linux(), XDialogTheme::Light, None);
        let mut ws = MockWs { ppp: 1.0, ..Default::default() };
        // expected_ppp() = 1.0 (default) and the window reports 1.0: no resize.
        show(&mut m, &mut ws, 1, &["OK"]);
        assert!(!ws.log.iter().any(|l| l.starts_with("size")));
        let mut ws = MockWs { ppp: 1.5, ..Default::default() };
        show(&mut m, &mut ws, 2, &["OK"]);
        assert!(!ws.log.iter().any(|l| l.starts_with("size")), "created at the right physical size: {:?}", ws.log);
        assert_eq!(m.desired_physical_size(2, 1.5), m.dialog(2).map(|d| d.size_px()));
    }
}
