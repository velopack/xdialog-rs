//! The core of the drawn backends (Fluent, Ubuntu): runtime and event loop, windows, input,
//! appearance/DPI, accessibility (AccessKit), request handling and test hooks, plus the measure
//! pass, the keyboard policy, text layout and a small widget API ([`ui::Ui`]). It draws through
//! the `draw` layer (a display list replayed by the one drawing backend of the target). Look,
//! layout and widgets live in the themes.

pub(crate) mod a11y;
pub(crate) mod anim;
pub(crate) mod appearance;
#[cfg(draw_soft)]
pub(crate) mod background;
pub(crate) mod clock;
pub(crate) mod dialog;
pub(crate) mod event_loop;
pub(crate) mod input;
pub(crate) mod keyboard;
pub(crate) mod runtime;
pub(crate) mod text;
pub(crate) mod theme;
pub(crate) mod ui;

#[cfg(windows)]
pub(crate) mod platform_win;

#[cfg(feature = "_test-hooks")]
pub(crate) mod offscreen;
