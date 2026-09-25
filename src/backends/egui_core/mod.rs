//! The egui backend core: plumbing (runtime and event loop, windows, egui-winit input,
//! fonts/bidi, presenter, appearance/DPI, request handling, test hooks) plus the measure pass and
//! the keyboard policy. Look, layout and widgets live in the themes, which build each dialog from
//! egui layout and their own `egui::Widget`s (egui does hover, press and focus).

pub(crate) mod anim;
pub(crate) mod appearance;
#[cfg(target_os = "linux")]
pub(crate) mod background;
pub(crate) mod bidi;
pub(crate) mod clock;
pub(crate) mod color;
pub(crate) mod dialog;
pub(crate) mod event_loop;
pub(crate) mod fonts;
pub(crate) mod keyboard;
pub(crate) mod render;
pub(crate) mod runtime;
pub(crate) mod text;
pub(crate) mod theme;

#[cfg(windows)]
pub(crate) mod platform_win;

#[cfg(feature = "_test-hooks")]
pub(crate) mod offscreen;
