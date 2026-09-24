//! winit-host mode: xdialog runs inside an event loop the application
//! owns. The public surface is `backends/host_api.rs` (`xdialog::host`), which delegates here.
//!
//! Pieces:
//! - [`Shared`] (process-wide, `Send + Sync`): the request queue, the coalescing wake flag, the
//!   host's waker, the bound host thread and the shut-down flag. [`HostHandler`] is the
//!   `DialogRequestHandler` installed by [`init_winit_host`]; it pushes to the queue and wakes the
//!   host (`waker()` only when `wake_pending` flips from `false` to `true`).
//! - [`HostState`] (host thread only, in a `thread_local!`): the [`Manager`] with its softbuffer
//!   presenters (raw handles, hence not `Send`) and the table of host windows. It is created by the
//!   first [`pump`], which also binds the host thread and marks it as an xdialog UI thread.
//! - [`HostWs`]: the `WindowSystem` over the host's `&mut dyn HostWindows`, created per call.
//!
//! Protocols:
//! - **Wake.** `pump` clears `wake_pending` BEFORE draining, so a request that arrives during or
//!   after the drain always produces a fresh `waker()` call; nothing is stranded.
//! - **Deadlines after input.** `handle_event` / `redraw` can schedule a frame after the host
//!   already chose `ControlFlow::Wait`. They fire due timers themselves and, when the earliest
//!   deadline is sooner than the one `pump` last returned, wake the host (same coalescing flag).
//! - **Re-entrancy.** Every entry point takes the state with `try_borrow_mut`. A nested call (a
//!   `HostWindows` impl or toolkit calling back into `xdialog::host` synchronously, or a progress
//!   callback) is recorded and executed by the outer entry point before it returns, with the outer
//!   call's `HostWindows`. It never panics with `BorrowMutError`.
//! - **Panics.** Every body runs under `catch_unwind`; a panic never unwinds into the host's loop.
//!   The dialog being processed is closed (`WindowClosed`, window destroyed); others continue.
//! - **Shutdown.** Closes every dialog (waiters get `WindowClosed`), drops all presenters, calls
//!   `destroy_window` for each window, drops the manager and fails every queued and future request
//!   with `NoBackendAvailable`. A host thread that exits with a live manager (the host forgot
//!   `shutdown`) leaks it instead of dropping surfaces against a possibly closed display.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::ThreadId;
use std::time::Instant;

use egui::Vec2;
use raw_window_handle::RawWindowHandle;

use super::manager::{request_id, Manager};
use super::render::{Presenter, RawHandles, SoftwarePresenter};
use super::window_system::{CreatedWindow, WindowSpec, WindowSystem};
use crate::backends::answer_with_error;
use crate::backends::host_types::{HostEvent, HostWindows, WindowKey, WindowRequest};
use crate::backends::linux_egui::LinuxTheme;
use crate::channel::{init_handler, mark_ui_thread, DialogRequestHandler};
use crate::model::DialogMessageRequest;
use crate::{XDialogError, XDialogTheme};

type BoxedWaker = Box<dyn Fn() + Send + Sync + 'static>;

// -------------------------------------------------------------------------------------------------
// Process-wide state
// -------------------------------------------------------------------------------------------------

/// State shared between the host thread and every thread that sends dialog requests.
struct Shared {
    /// Pending requests and the shut-down flag (one lock, so a request is either queued before
    /// shutdown drains the queue, or rejected).
    queue: Mutex<Queue>,
    /// A `waker()` call is outstanding (cleared by `pump` before it drains).
    wake_pending: AtomicBool,
    waker: BoxedWaker,
    /// The thread of the first `pump` (the host thread).
    host_thread: OnceLock<ThreadId>,
    /// Fonts or the system appearance changed on a background thread (`Manager::refresh`).
    refresh_pending: AtomicBool,
    xtheme: XDialogTheme,
}

#[derive(Default)]
struct Queue {
    requests: VecDeque<DialogMessageRequest>,
    #[cfg(xd_test_hooks)]
    remote: VecDeque<super::manager::live::RemoteCmd>,
    shut_down: bool,
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn is_shut_down(&self) -> bool {
        self.lock().shut_down
    }

    /// Whether the current thread is the bound host thread (and xdialog is still running there).
    fn is_host_thread(&self) -> bool {
        self.host_thread.get().is_some_and(|t| *t == std::thread::current().id()) && crate::channel::is_ui_thread()
    }

    /// Call the host's waker unless a wake is already outstanding. Never unwinds into the caller.
    /// A panicking waker did not wake the host, so the flag is cleared again: the next request
    /// retries the waker instead of assuming a pump is on its way.
    fn wake(&self) {
        if !self.wake_pending.swap(true, Ordering::SeqCst) && catch_unwind(AssertUnwindSafe(|| (self.waker)())).is_err() {
            self.wake_pending.store(false, Ordering::SeqCst);
            error!("xdialog::host: the waker passed to init_winit_host panicked");
        }
    }

    /// Reject everything queued and everything sent from now on with `NoBackendAvailable`.
    fn shut_down(&self) {
        let pending = {
            let mut q = self.lock();
            q.shut_down = true;
            #[cfg(xd_test_hooks)]
            q.remote.clear();
            std::mem::take(&mut q.requests)
        };
        for msg in pending {
            answer_with_error(msg, &|| XDialogError::NoBackendAvailable);
        }
    }
}

/// The request handler installed by [`init_winit_host`].
struct HostHandler(Arc<Shared>);

impl DialogRequestHandler for HostHandler {
    fn send(&self, message: DialogMessageRequest) -> Result<(), XDialogError> {
        // On the host thread a creation can only complete in a later `pump`, so a caller waiting for
        // it here would deadlock the loop that has to produce it. `message.rs` /
        // `progress.rs` guard this too; answering here keeps host mode safe on its own.
        let message = if self.0.is_host_thread() { self.on_host_thread(message) } else { Some(message) };
        let Some(message) = message else { return Ok(()) };
        {
            let mut q = self.0.lock();
            if q.shut_down {
                drop(q);
                answer_with_error(message, &|| XDialogError::NoBackendAvailable);
                return Ok(());
            }
            q.requests.push_back(message);
        }
        self.0.wake();
        Ok(())
    }
}

impl HostHandler {
    /// A request sent on the host thread: a message box fails with `BlockingCallOnUiThread`; a
    /// progress dialog is acknowledged now (the window appears on the next `pump`; a creation error
    /// is then only logged). Returns the request to queue, if any.
    fn on_host_thread(&self, message: DialogMessageRequest) -> Option<DialogMessageRequest> {
        match message {
            DialogMessageRequest::ShowMessageWindow(_, _, creation) => {
                warn!("xdialog::host: a blocking message dialog was requested on the host event-loop thread; call it from another thread");
                let _ = creation.send(Err(XDialogError::BlockingCallOnUiThread));
                None
            }
            DialogMessageRequest::ShowProgressWindow(id, options, creation, callback) => {
                // The caller only needs the acknowledgement (progress results are not waited for).
                let (_result_tx, result_rx) = oneshot::channel();
                let _ = creation.send(Ok(result_rx));
                let (creation, _ack) = oneshot::channel();
                Some(DialogMessageRequest::ShowProgressWindow(id, options, creation, callback))
            }
            other => Some(other),
        }
    }
}

pub(crate) fn init_winit_host(theme: XDialogTheme, waker: BoxedWaker) -> Result<(), XDialogError> {
    let shared = Arc::new(Shared { queue: Mutex::new(Queue::default()),
                                   wake_pending: AtomicBool::new(false),
                                   waker,
                                   host_thread: OnceLock::new(),
                                   refresh_pending: AtomicBool::new(false),
                                   xtheme: theme });
    if SHARED.get().is_some() || !init_handler(Box::new(HostHandler(shared.clone()))) {
        let msg = "xdialog: init_winit_host: a dialog backend was already initialized (another init_* call or an XDialogBuilder ran first)";
        return Err(XDialogError::SystemError(msg.into()));
    }
    // `init_handler` accepted exactly one handler per process, so this is the only `set`.
    let _ = SHARED.set(shared);
    Ok(())
}

// -------------------------------------------------------------------------------------------------
// Host-thread state
// -------------------------------------------------------------------------------------------------

/// Per-window data the window system needs.
struct WinInfo {
    handles: RawHandles,
}

/// The host windows xdialog owns, plus what it learned about the host's monitors.
#[derive(Default)]
struct WinTable {
    windows: HashMap<WindowKey, WinInfo>,
    next_key: u64,
    /// Scale factor of the most recently created window (measure pass of the next dialog).
    last_scale: Option<f64>,
    /// Work-area height (logical) the host reported with its most recent window.
    work_area: Option<f64>,
}

/// Everything xdialog keeps on the host thread.
struct HostState {
    manager: Manager<LinuxTheme, WindowKey>,
    table: WinTable,
    /// The deadline the last `pump` returned (`None` = wait indefinitely).
    last_deadline: Option<Instant>,
}

/// An entry-point call.
#[derive(Clone, Copy, Debug)]
enum Op {
    Pump,
    Event(WindowKey, HostEvent),
    Redraw(WindowKey),
    Shutdown,
}

impl Op {
    fn name(self) -> &'static str {
        match self {
            Op::Pump => "pump",
            Op::Event(..) => "handle_event",
            Op::Redraw(_) => "redraw",
            Op::Shutdown => "shutdown",
        }
    }
}

/// The thread-local slot. Its destructor leaks a live manager (the host did not call `shutdown`):
/// dropping softbuffer surfaces against a display connection that may already be closed is UB.
struct Slot {
    state: RefCell<Option<HostState>>,
    /// Entry-point calls that arrived while `state` was borrowed (re-entrancy).
    deferred: RefCell<VecDeque<Op>>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Some(state) = self.state.get_mut().take() {
            error!("xdialog::host: the host thread exited without calling xdialog::host::shutdown; leaking {} dialog window(s)",
                   state.manager.len());
            std::mem::forget(state);
            if let Some(shared) = SHARED.get() {
                shared.shut_down();
            }
        }
    }
}

thread_local! {
    static SLOT: Slot = const { Slot { state: RefCell::new(None), deferred: RefCell::new(VecDeque::new()) } };
}

// -------------------------------------------------------------------------------------------------
// Window system over the host's `HostWindows`
// -------------------------------------------------------------------------------------------------

struct HostWs<'a> {
    host: &'a mut dyn HostWindows,
    table: &'a mut WinTable,
}

impl HostWs<'_> {
    fn presenter(handles: RawHandles) -> Result<Box<dyn Presenter>, String> {
        // SAFETY: `HostWindows` (an `unsafe trait`) guarantees that the window stays valid until
        // xdialog calls `destroy_window` for it and the display until the last `destroy_window`
        // returned. The manager drops the presenter before `WindowSystem::destroy`, which is the
        // only place `destroy_window` is called for a window that has a presenter.
        let p = unsafe { SoftwarePresenter::from_raw(handles) }.map_err(|e| e.to_string())?;
        Ok(Box::new(p))
    }
}

impl WindowSystem for HostWs<'_> {
    type Win = WindowKey;

    fn create(&mut self, spec: &WindowSpec<'_>) -> Result<CreatedWindow<WindowKey>, XDialogError> {
        self.table.next_key += 1;
        let key = WindowKey::from_raw(self.table.next_key);
        let request = WindowRequest { key,
                                      title: spec.title.to_owned(),
                                      width: spec.inner_size.x as f64,
                                      height: spec.inner_size.y as f64,
                                      visible: false,
                                      active: spec.active,
                                      dark: spec.dark_titlebar,
                                      follow_system: spec.follow_system,
                                      resizable: false,
                                      position: spec.position.map(|[x, y]| (x, y)) };
        let hw = self.host
                     .create_window(&request)
                     .map_err(|e| XDialogError::SystemError(format!("xdialog: the host could not create a window: {e}")))?;
        let handles = RawHandles { display: hw.display_handle(), window: hw.window_handle() };
        let presenter = match Self::presenter(handles) {
            Ok(p) => p,
            Err(e) => {
                self.host.destroy_window(key);
                return Err(XDialogError::SystemError(format!("xdialog: could not create a surface for the host window: {e}")));
            }
        };
        let scale = hw.scale_factor();
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        self.table.last_scale = Some(scale);
        if let Some(h) = hw.work_area_height().filter(|h| h.is_finite() && *h > 0.0) {
            self.table.work_area = Some(h);
        }
        self.table.windows.insert(key, WinInfo { handles });
        let (w, h) = hw.inner_size();
        Ok(CreatedWindow { win: key, presenter, ppp: scale as f32, size_px: [w, h] })
    }

    fn set_visible(&mut self, w: &WindowKey, visible: bool) {
        self.host.set_visible(*w, visible);
    }

    fn request_inner_size(&mut self, w: &WindowKey, logical: Vec2) -> Option<[u32; 2]> {
        // The host forwards the window's `Resized` event.
        self.host.set_inner_size(*w, logical.x as f64, logical.y as f64);
        None
    }

    fn request_redraw(&mut self, w: &WindowKey) {
        self.host.request_redraw(*w);
    }

    fn set_dark_titlebar(&mut self, w: &WindowKey, dark: bool) {
        self.host.set_dark(*w, dark);
    }

    fn destroy(&mut self, w: WindowKey) {
        self.table.windows.remove(&w);
        self.host.destroy_window(w);
    }

    fn max_client_height(&self) -> Option<f32> {
        self.table.work_area.map(|h| (h * 0.9) as f32)
    }

    fn expected_ppp(&self) -> f32 {
        self.table.last_scale.unwrap_or(1.0) as f32
    }

    fn raw_window_id(&self, w: &WindowKey) -> isize {
        match self.table.windows.get(w).map(|i| i.handles.window) {
            Some(RawWindowHandle::Win32(h)) => h.hwnd.get(),
            Some(RawWindowHandle::Xlib(h)) => h.window as isize,
            Some(RawWindowHandle::Xcb(h)) => h.window.get() as isize,
            _ => 0,
        }
    }

    fn recreate_presenter(&mut self, w: &WindowKey) -> Option<Box<dyn Presenter>> {
        let handles = self.table.windows.get(w)?.handles;
        match Self::presenter(handles) {
            Ok(p) => Some(p),
            Err(e) => {
                warn!("xdialog::host: could not recreate the surface: {e}");
                None
            }
        }
    }

    fn virtual_screen_left(&self) -> i32 {
        #[cfg(windows)]
        {
            super::platform_win::virtual_screen_left()
        }
        #[cfg(not(windows))]
        {
            0
        }
    }
}

// -------------------------------------------------------------------------------------------------
// Entry points
// -------------------------------------------------------------------------------------------------

pub(crate) fn pump(host: &mut dyn HostWindows) -> Option<Instant> {
    enter(host, Op::Pump)
}

pub(crate) fn handle_event(host: &mut dyn HostWindows, key: WindowKey, event: HostEvent) {
    enter(host, Op::Event(key, event));
}

pub(crate) fn redraw(host: &mut dyn HostWindows, key: WindowKey) {
    enter(host, Op::Redraw(key));
}

pub(crate) fn shutdown(host: &mut dyn HostWindows) {
    enter(host, Op::Shutdown);
}

/// Common entry: initialization and thread checks, re-entrancy, then [`run`] plus every call
/// deferred while it ran.
fn enter(host: &mut dyn HostWindows, op: Op) -> Option<Instant> {
    let shared = SHARED.get()?.clone();
    let here = std::thread::current().id();
    let bound = match op {
        Op::Pump => *shared.host_thread.get_or_init(|| here),
        _ => match shared.host_thread.get() {
            Some(t) => *t,
            None => {
                // Nothing was created yet (no pump so far): only shutdown has an effect.
                if matches!(op, Op::Shutdown) {
                    shared.shut_down();
                }
                return None;
            }
        },
    };
    if bound != here {
        wrong_thread(&shared, op);
        return None;
    }

    let entered = SLOT.try_with(|slot| {
                          let Ok(mut state) = slot.state.try_borrow_mut() else {
                              // Re-entrant call: the outer entry point runs it before returning.
                              slot.deferred.borrow_mut().push_back(op);
                              return None;
                          };
                          if state.is_none() && matches!(op, Op::Pump) && !shared.is_shut_down() {
                              match catch_unwind(AssertUnwindSafe(|| new_state(&shared))) {
                                  Ok(s) => *state = Some(s),
                                  Err(_) => {
                                      error!("xdialog::host: starting host mode panicked; dialogs are unavailable");
                                      mark_ui_thread(false);
                                      shared.shut_down();
                                      return None;
                                  }
                              }
                          }
                          let result = run(&shared, &mut state, host, op);
                          loop {
                              let next = slot.deferred.borrow_mut().pop_front();
                              let Some(op) = next else { break };
                              let _ = run(&shared, &mut state, host, op);
                          }
                          result
                      });
    entered.unwrap_or_else(|_| {
               // The thread-local is being destroyed (host thread exiting).
               if matches!(op, Op::Shutdown) {
                   shared.shut_down();
               }
               None
           })
}

/// First `pump` on the host thread: mark it as a UI thread, create the manager.
fn new_state(shared: &Arc<Shared>) -> HostState {
    mark_ui_thread(true);
    let refresh = Arc::downgrade(shared);
    let waker: super::fonts::Waker = Arc::new(move || {
        if let Some(s) = refresh.upgrade() {
            s.refresh_pending.store(true, Ordering::SeqCst);
            if !s.is_shut_down() {
                s.wake();
            }
        }
    });
    #[allow(unused_mut)]
    let mut manager = Manager::new(LinuxTheme::new(), shared.xtheme.clone(), Some(waker));
    #[cfg(xd_test_hooks)]
    {
        let remote = Arc::downgrade(shared);
        manager.set_remote(Arc::new(move |cmd| {
                               if let Some(s) = remote.upgrade() {
                                   {
                                       let mut q = s.lock();
                                       if q.shut_down {
                                           return;
                                       }
                                       q.remote.push_back(cmd);
                                   }
                                   s.wake();
                               }
                           }));
    }
    HostState { manager, table: WinTable::default(), last_deadline: None }
}

/// A host-mode function was called on another thread than the bound host thread.
fn wrong_thread(shared: &Shared, op: Op) {
    let name = op.name();
    error!("xdialog::host::{name} called from a different thread than the first xdialog::host::pump; all xdialog::host functions must \
            run on the host event-loop thread");
    if matches!(op, Op::Pump) {
        // Fail queued creations so blocked callers get an answer instead of hanging; keep updates
        // for existing dialogs.
        let pending = std::mem::take(&mut shared.lock().requests);
        let mut keep = VecDeque::new();
        for msg in pending {
            match msg {
                DialogMessageRequest::ShowMessageWindow(..) | DialogMessageRequest::ShowProgressWindow(..) => {
                    answer_with_error(msg, &|| {
                        XDialogError::SystemError("xdialog::host::pump called from a different thread than the first pump".into())
                    });
                }
                other => keep.push_back(other),
            }
        }
        let mut q = shared.lock();
        keep.extend(q.requests.drain(..));
        q.requests = keep;
    }
    debug_assert!(false, "xdialog::host::{name} called from a different thread than the first xdialog::host::pump");
}

/// Execute one entry-point call with the state borrowed.
fn run(shared: &Shared, slot: &mut Option<HostState>, host: &mut dyn HostWindows, op: Op) -> Option<Instant> {
    match op {
        Op::Shutdown => {
            do_shutdown(shared, slot, host);
            None
        }
        Op::Pump => {
            let state = slot.as_mut()?;
            do_pump(shared, state, host)
        }
        Op::Event(key, ev) => {
            let state = slot.as_mut()?;
            let id = state.manager.dialog_for(|w| *w == key)?;
            guarded(state, host, Some(id), "handle_event", |m, ws| m.handle_event(ws, id, ev));
            after_input(shared, state, host);
            None
        }
        Op::Redraw(key) => {
            let state = slot.as_mut()?;
            let id = state.manager.dialog_for(|w| *w == key)?;
            guarded(state, host, Some(id), "redraw", |m, ws| m.redraw(ws, id));
            after_input(shared, state, host);
            None
        }
    }
}

/// Run `f` on the manager under `catch_unwind`. On a panic, close dialog `id` (if known).
fn guarded(state: &mut HostState,
           host: &mut dyn HostWindows,
           id: Option<usize>,
           what: &str,
           f: impl FnOnce(&mut Manager<LinuxTheme, WindowKey>, &mut HostWs<'_>)) {
    let HostState { manager, table, .. } = state;
    let mut ws = HostWs { host: &mut *host, table };
    if catch_unwind(AssertUnwindSafe(|| f(manager, &mut ws))).is_ok() {
        return;
    }
    error!("xdialog::host: {what} panicked");
    let Some(id) = id else { return };
    let closed = catch_unwind(AssertUnwindSafe(|| {
                                  manager.handle_request(&mut ws, DialogMessageRequest::CloseWindow(id));
                              }));
    if closed.is_err() {
        error!("xdialog::host: closing dialog {id} after a panic panicked again");
    }
}

fn do_pump(shared: &Shared, state: &mut HostState, host: &mut dyn HostWindows) -> Option<Instant> {
    // 1. Clear the flag BEFORE draining: anything sent from now on wakes the host again.
    shared.wake_pending.store(false, Ordering::SeqCst);

    // 2. Drain until the queue is observed empty (one request at a time, outside the lock).
    loop {
        let msg = shared.lock().requests.pop_front();
        let Some(msg) = msg else { break };
        let id = request_id(&msg);
        // `ExitEventLoop` (never sent in host mode) only closes every dialog: the host owns its loop.
        guarded(state, host, id, "handling a dialog request", |m, ws| {
            let _ = m.handle_request(ws, msg);
        });
    }

    #[cfg(xd_test_hooks)]
    loop {
        let cmd = shared.lock().remote.pop_front();
        let Some(cmd) = cmd else { break };
        guarded(state, host, Some(cmd.id()), "a test-hook command", |m, ws| m.handle_remote(ws, cmd));
    }

    if shared.refresh_pending.swap(false, Ordering::SeqCst) {
        guarded(state, host, None, "refreshing fonts/appearance", |m, ws| m.refresh(ws));
    }

    // 3. Fire due timers.
    let now = Instant::now();
    let mut next = None;
    guarded(state, host, None, "polling timers", |m, ws| next = m.poll_timers(ws, now));

    // 4. Something arrived after the drain: ask to be pumped again right away.
    if !shared.lock().requests.is_empty() || shared.refresh_pending.load(Ordering::SeqCst) {
        next = Some(now);
    }
    state.last_deadline = next;
    next
}

/// After `handle_event` / `redraw`: fire due timers now (an immediate repaint needs no extra
/// iteration) and wake the host when a deadline is sooner than what the last `pump` returned.
fn after_input(shared: &Shared, state: &mut HostState, host: &mut dyn HostWindows) {
    let mut next = None;
    guarded(state, host, None, "polling timers", |m, ws| next = m.poll_timers(ws, Instant::now()));
    if let Some(t) = next {
        if state.last_deadline.is_none_or(|last| t < last) {
            state.last_deadline = Some(t);
            shared.wake();
        }
    }
}

fn do_shutdown(shared: &Shared, slot: &mut Option<HostState>, host: &mut dyn HostWindows) {
    // New and queued requests fail from here on; blocked callers return immediately.
    shared.shut_down();
    let Some(mut state) = slot.take() else { return };
    guarded(&mut state, host, None, "shutdown", |m, ws| m.close_all(ws));
    // Anything the manager did not manage to close: release surfaces first, then the windows.
    let HostState { manager, mut table, .. } = state;
    if catch_unwind(AssertUnwindSafe(|| drop(manager))).is_err() {
        error!("xdialog::host: dropping the dialog manager panicked");
    }
    for (key, _) in table.windows.drain() {
        host.destroy_window(key);
    }
    mark_ui_thread(false);
}
