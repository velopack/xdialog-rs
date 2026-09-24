//! Version-neutral window and input types.
//!
//! These cross the boundary between xdialog and a host application (`xdialog::host`) or the test
//! hooks (`xdialog::__test`). They deliberately import nothing from egui or winit, so they compile
//! on every OS (including the macOS `host` stub) and never tie the public API to a winit version.
//! Only `raw-window-handle` 0.6 appears in the public surface.

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

/// A version-neutral window event. Coordinates and sizes are PHYSICAL pixels, client-relative.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum HostEvent {
    /// The client area was resized (physical pixels). A zero side means minimised.
    Resized {
        /// New client width in physical pixels.
        width: u32,
        /// New client height in physical pixels.
        height: u32,
    },
    /// The window's DPI scale factor changed.
    ScaleFactorChanged {
        /// The new scale factor (physical pixels per logical pixel).
        scale_factor: f64,
    },
    /// The pointer moved inside the window (physical pixels, client-relative).
    CursorMoved {
        /// Horizontal position in physical pixels.
        x: f64,
        /// Vertical position in physical pixels.
        y: f64,
    },
    /// The pointer left the window.
    CursorLeft,
    /// A mouse button was pressed or released.
    MouseButton {
        /// Which button.
        button: MouseButton,
        /// `true` on press, `false` on release.
        pressed: bool,
    },
    /// Mouse wheel / touchpad scroll (winit `MouseWheel`).
    MouseWheel {
        /// The scroll amount.
        delta: ScrollDelta,
    },
    /// Touch contact. xdialog maps the PRIMARY touch (first id down while none is active) to
    /// pointer move + primary press/release; other ids are ignored.
    Touch {
        /// Touch identifier (stable for the duration of one contact).
        id: u64,
        /// Phase of the contact.
        phase: TouchPhase,
        /// Horizontal position in physical pixels.
        x: f64,
        /// Vertical position in physical pixels.
        y: f64,
    },
    /// A key was pressed or released. Forward only non-synthetic key events.
    Key {
        /// Which key.
        key: Key,
        /// `true` on press, `false` on release.
        pressed: bool,
        /// `true` for auto-repeat presses.
        repeat: bool,
    },
    /// The keyboard modifier state changed.
    Modifiers(Modifiers),
    /// The window gained (`true`) or lost (`false`) keyboard focus.
    Focused(bool),
    /// The system light/dark theme changed.
    ThemeChanged,
    /// The user asked to close the window (title-bar close button, Alt+F4, ...). Do not close the
    /// window yourself; xdialog calls `destroy_window`.
    CloseRequested,
}

/// A scroll amount.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum ScrollDelta {
    /// Scroll by lines (mouse wheel notches).
    Lines {
        /// Horizontal lines.
        x: f32,
        /// Vertical lines.
        y: f32,
    },
    /// Scroll by physical pixels (touchpads).
    Pixels {
        /// Horizontal pixels.
        x: f64,
        /// Vertical pixels.
        y: f64,
    },
}

/// Phase of a touch contact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TouchPhase {
    /// The finger touched the screen.
    Started,
    /// The finger moved.
    Moved,
    /// The finger was lifted.
    Ended,
    /// The system cancelled the contact.
    Cancelled,
}

/// The keys xdialog reacts to. Map other keys to nothing (don't forward them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Key {
    /// Enter / Return.
    Enter,
    /// Escape.
    Escape,
    /// Tab (Shift+Tab via [`Modifiers`]).
    Tab,
    /// Space bar.
    Space,
    /// Left arrow.
    ArrowLeft,
    /// Right arrow.
    ArrowRight,
    /// Up arrow.
    ArrowUp,
    /// Down arrow.
    ArrowDown,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MouseButton {
    /// The primary (usually left) button.
    Primary,
    /// The secondary (usually right) button.
    Secondary,
    /// The middle button.
    Middle,
    /// Any other button.
    Other,
}

/// Keyboard modifier state. Private fields so new modifiers can be added without a breaking change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    shift: bool,
    ctrl: bool,
    alt: bool,
    logo: bool,
}

impl Modifiers {
    /// Create a modifier state.
    pub const fn new(shift: bool, ctrl: bool, alt: bool, logo: bool) -> Self {
        Modifiers { shift, ctrl, alt, logo }
    }
    /// Shift is held.
    pub fn shift(&self) -> bool {
        self.shift
    }
    /// Control is held.
    pub fn ctrl(&self) -> bool {
        self.ctrl
    }
    /// Alt / Option is held.
    pub fn alt(&self) -> bool {
        self.alt
    }
    /// The logo key (Windows / Super / Command) is held.
    pub fn logo(&self) -> bool {
        self.logo
    }
}

/// Identifies a window xdialog asked the host to create.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowKey(u64);

impl WindowKey {
    /// Crate-internal constructor (keys are allocated by xdialog).
    #[allow(dead_code)]
    pub(crate) const fn from_raw(raw: u64) -> Self {
        WindowKey(raw)
    }
    /// Crate-internal raw value.
    #[allow(dead_code)]
    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
}

/// Parameters for a window xdialog needs. Sizes are logical pixels.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct WindowRequest {
    /// The key xdialog will use to refer to this window.
    pub key: WindowKey,
    /// Window title.
    pub title: String,
    /// Inner (client) width in logical pixels.
    pub width: f64,
    /// Inner (client) height in logical pixels.
    pub height: f64,
    /// Create the window hidden; xdialog calls `set_visible(true)` after its first frame.
    /// Always `false` today.
    pub visible: bool,
    /// Whether the window may take focus when shown (`false` under `XDIALOG_TEST_NO_ACTIVATE`).
    pub active: bool,
    /// Request a dark title bar / decorations.
    pub dark: bool,
    /// The dialog follows the system light/dark preference (`XDialogTheme::SystemDefault`).
    /// Create the window without a fixed theme then (winit: `with_theme(None)`): winit on Windows
    /// reports `WindowEvent::ThemeChanged` only for windows without a preferred theme, and
    /// xdialog needs that event ([`HostEvent::ThemeChanged`]) to follow a light/dark switch.
    pub follow_system: bool,
    /// Whether the window may be resized by the user. Always `false` today: the window should
    /// only offer a close button.
    pub resizable: bool,
    /// Suggested physical position; `None` = centre it (recommended: on the primary monitor).
    pub position: Option<(i32, i32)>,
}

/// Raw handles and current metrics of a window created by the host.
#[derive(Clone, Copy, Debug)]
pub struct HostWindow {
    display: RawDisplayHandle,
    window: RawWindowHandle,
    scale_factor: f64,
    inner_size: (u32, u32),
    work_area_height: Option<f64>,
}

impl HostWindow {
    /// Describe a window the host created. `inner_size` is in physical pixels.
    pub fn new(display: RawDisplayHandle, window: RawWindowHandle, scale_factor: f64, inner_size: (u32, u32)) -> Self {
        HostWindow { display, window, scale_factor, inner_size, work_area_height: None }
    }

    /// Optional: the monitor work-area height in LOGICAL pixels, used to cap the dialog height
    /// (the cap is 800 when not given).
    pub fn with_work_area_height(mut self, logical_height: f64) -> Self {
        self.work_area_height = Some(logical_height);
        self
    }

    #[allow(dead_code)]
    pub(crate) fn display_handle(&self) -> RawDisplayHandle {
        self.display
    }
    #[allow(dead_code)]
    pub(crate) fn window_handle(&self) -> RawWindowHandle {
        self.window
    }
    #[allow(dead_code)]
    pub(crate) fn scale_factor(&self) -> f64 {
        self.scale_factor
    }
    #[allow(dead_code)]
    pub(crate) fn inner_size(&self) -> (u32, u32) {
        self.inner_size
    }
    #[allow(dead_code)]
    pub(crate) fn work_area_height(&self) -> Option<f64> {
        self.work_area_height
    }
}

/// Window operations the host performs for xdialog. Borrowed by xdialog only for the duration of a
/// `pump` / `handle_event` / `redraw` / `shutdown` call, so it can wrap a borrowed event-loop target.
///
/// # Safety
/// - The window handle returned from `create_window` must stay valid until xdialog calls
///   `destroy_window` for that key (xdialog releases its surface before that call). The window must
///   only be destroyed by `destroy_window`.
/// - The **display** connection behind the returned display handle (X11 `Display`/xcb connection,
///   Wayland `wl_display`) must stay open until the last `destroy_window` call has returned.
/// - `xdialog::host::shutdown` must be called before the implementor's windows, display connection
///   or event loop are dropped. Dropping them first is undefined behaviour (softbuffer detaches
///   shared memory and frees GCs on the host's X11 connection when its surfaces drop).
pub unsafe trait HostWindows {
    /// Create a window as described by `request` and return its handles, or an error message.
    fn create_window(&mut self, request: &WindowRequest) -> Result<HostWindow, String>;
    /// Show or hide the window.
    fn set_visible(&mut self, key: WindowKey, visible: bool);
    /// Request a new inner size in logical pixels; the host should apply it (e.g.
    /// `request_inner_size`) and later forward `Resized`.
    fn set_inner_size(&mut self, key: WindowKey, width: f64, height: f64);
    /// Ask the host to deliver a `RedrawRequested` for this window (forward it to `redraw`).
    fn request_redraw(&mut self, key: WindowKey);
    /// Destroy the window. xdialog has already released its surface.
    fn destroy_window(&mut self, key: WindowKey);
    /// Switch the window's title bar / decorations to dark (`true`) or light, like
    /// [`WindowRequest::dark`] at creation; called when the appearance changes afterwards (system
    /// theme change). Optional: the default does nothing, leaving the creation colour.
    fn set_dark(&mut self, key: WindowKey, dark: bool) {
        let _ = (key, dark);
    }
}
