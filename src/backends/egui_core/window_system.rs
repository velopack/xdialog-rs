//! Window-system abstraction. Implemented by the own winit loop (`own_loop.rs`) and the host
//! integration (`host.rs`). The offscreen path does not use it: it drives `Dialog` directly with a
//! `MemoryPresenter`.

use egui::Vec2;

use super::render::Presenter;
use crate::XDialogError;

/// What core asks the window system to create.
#[derive(Clone, Debug)]
pub(crate) struct WindowSpec<'a> {
    pub title: &'a str,
    /// Logical inner size.
    pub inner_size: Vec2,
    pub dark_titlebar: bool,
    /// The dialog follows the system light/dark preference (`XDialogTheme::SystemDefault`): the
    /// window must not pin a theme, or winit on Windows never reports `ThemeChanged`.
    pub follow_system: bool,
    /// `false` under `XDIALOG_TEST_NO_ACTIVATE` (never take focus).
    pub active: bool,
    /// Physical top-left; `XDIALOG_TEST_POS`, or `None` = centred on the primary monitor.
    pub position: Option<[i32; 2]>,
}

/// A created (still invisible) window plus its presenter and initial metrics.
pub(crate) struct CreatedWindow<W> {
    pub win: W,
    pub presenter: Box<dyn Presenter>,
    pub ppp: f32,
    pub size_px: [u32; 2],
}

pub(crate) trait WindowSystem {
    type Win;
    /// Create an INVISIBLE window at the exact logical size.
    fn create(&mut self, spec: &WindowSpec<'_>) -> Result<CreatedWindow<Self::Win>, XDialogError>;
    fn set_visible(&mut self, w: &Self::Win, visible: bool);
    /// Ask for a new logical client size. Returns the new PHYSICAL client size when the window
    /// system applied it immediately and may not report a resize event for it (winit on
    /// Wayland); `None` when a resize event will follow (or nothing changed).
    fn request_inner_size(&mut self, w: &Self::Win, logical: Vec2) -> Option<[u32; 2]>;
    fn request_redraw(&mut self, w: &Self::Win);
    /// The presenter was dropped by the Manager BEFORE this is called.
    fn destroy(&mut self, w: Self::Win);
    /// Logical max client height (work area * 0.9); `None` when unknown (host mode).
    fn max_client_height(&self) -> Option<f32>;

    /// The scale factor a new window will most likely get (used for the measure pass before the
    /// window exists). Own loop: the primary monitor's scale.
    fn expected_ppp(&self) -> f32 {
        1.0
    }

    /// Native window id for test introspection (HWND on Windows, XID on X11, 0 otherwise).
    fn raw_window_id(&self, _w: &Self::Win) -> isize {
        0
    }

    /// A fresh presenter for `w` after `present` failed (surface lost). `None`: not supported, the
    /// dialog keeps its presenter and retries on the next frame.
    fn recreate_presenter(&mut self, _w: &Self::Win) -> Option<Box<dyn Presenter>> {
        None
    }

    /// Logical max client width (work area * 0.9); `None` when unknown.
    fn max_client_width(&self) -> Option<f32> {
        None
    }

    /// Left edge of the whole virtual desktop in physical px (`XDIALOG_TEST_POS=offscreen`).
    fn virtual_screen_left(&self) -> i32 {
        0
    }

    /// The appearance switched: update the title bar / decorations (dark or light).
    fn set_dark_titlebar(&mut self, _w: &Self::Win, _dark: bool) {}
}
