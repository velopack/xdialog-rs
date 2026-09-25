//! The reusable egui backend core: plumbing only (event loop, windows, input translation,
//! fonts/bidi, presenter, appearance/DPI, request handling, direct/host modes, test hooks) plus the
//! measure pass and the keyboard policy. Look, layout and widgets live in the themes, which build
//! each dialog from egui layout and their own `egui::Widget`s (egui does hover, press and focus).
//!
//! Dead-code policy: many items here are only reached from some entry points (own loop, direct
//! mode, `xdialog::host`, the test hooks) or from one theme, so partial feature sets leave some of
//! them unused. The lint is therefore only relaxed for partial builds; the full build (both themes,
//! own loop, direct, host and test hooks, i.e. `--all-features`) must stay free of dead code.
#![cfg_attr(not(all(xd_own_loop, xd_theme_ubuntu, xd_theme_fluent, xd_linux_direct, xd_winit_host, xd_test_hooks)),
            allow(dead_code))]

pub(crate) mod anim;
pub(crate) mod appearance;
pub(crate) mod bidi;
pub(crate) mod clock;
pub(crate) mod color;
pub(crate) mod dialog;
pub(crate) mod fonts;
pub(crate) mod input;
pub(crate) mod keyboard;
pub(crate) mod manager;
pub(crate) mod render;
pub(crate) mod testhooks;
pub(crate) mod text;
pub(crate) mod theme;
pub(crate) mod window_system;

#[cfg(windows)]
pub(crate) mod platform_win;

#[cfg(xd_own_loop)]
pub(crate) mod own_loop;

#[cfg(xd_linux_direct)]
pub(crate) mod direct;

#[cfg(xd_winit_host)]
pub(crate) mod host;

#[cfg(xd_test_hooks)]
pub(crate) mod offscreen;
