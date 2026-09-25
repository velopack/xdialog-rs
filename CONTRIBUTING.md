# Contributing to xdialog

The egui backends share one core (`src/backends/egui_core/`: the event loop and host runtime,
dialog windows, fonts, software presenter); each look (`egui_fluent`, `egui_ubuntu`) is a small
theme built from egui's own layout, painting, text and animation, driven by design tokens.

- `cargo test` runs the unit and integration tests; `--features winit-host,_test-hooks` adds the
  host-mode test (`tests/winit_host.rs`). On Windows set `XDIALOG_TEST_NO_ACTIVATE=1` and
  `XDIALOG_TEST_POS=offscreen` so test windows never take focus.
- `cargo test --release --features _test-hooks --test egui_offscreen` checks the deterministic
  offscreen renders in `tests/visual_references/egui/` (`XDIALOG_VISUAL_SEED=1` re-seeds them;
  the Fluent ones apply only when the local Segoe UI Variable has the byte length recorded in
  `tests/visual_references/egui/fluent/FONT_ID`).
- `cargo run --release --example egui_gallery --features _test-hooks -- --theme all` renders every
  dialog variant of both looks to `target/egui_gallery/<theme>/` plus a contact sheet
  (`--out <dir>`, `--filter <substr>`).
- `XDIALOG_VISUAL_SEED=1 cargo test --test visual_regression` re-seeds the screenshot references
  of `tests/visual_regression.rs` (Win32 TaskDialog on Windows, AppKit on macOS; it takes focus,
  `XDIALOG_VISUAL_TEST=1` compares).
- Hidden environment variables (testing only): `XDIALOG_BACKEND=auto|win32|fluent|ubuntu|appkit`
  overrides the backend choice; `XDIALOG_TEST_NO_ACTIVATE`, and in debug or `_test-hooks` builds
  `XDIALOG_TEST_POS` and `XDIALOG_TEST_ACCENT`.
