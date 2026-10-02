# Contributing to xdialog

The drawn backends share one core: `src/backends/gui/` (the event loop and host runtime, dialog
state, input, animation, accessibility) records each frame into the display list of
`src/backends/draw/`, which one renderer per OS replays (`d2d`: Direct2D/DirectWrite on Windows,
`cg`: CoreGraphics/CoreText on macOS, `soft`: vello_cpu + cosmic-text elsewhere, with the bundled
Ubuntu fonts under the Ubuntu Font Licence in `src/backends/draw/soft/fonts/`). Each look
(`src/backends/fluent/`, `src/backends/ubuntu/`, `src/backends/macos/`) is a small theme over that
core, driven by design tokens. On macOS the `MacOS` look's window is transparent over an
`NSVisualEffectView` (`src/backends/gui/platform_mac.rs`) and presents through its own `CALayer`
(`src/backends/draw/cg/window.rs`); offscreen renders of it are opaque.

- `cargo test` runs the unit and integration tests (`tests/builder.rs`: the `XDialogBuilder::run*`
  life cycle); `--features winit-host,_test-hooks` adds the host-mode tests
  (`tests/winit_host.rs`, `tests/winit_host_on_demand.rs`), and `_test-hooks` the TaskDialog
  fallback (`tests/fallback.rs`, Windows). On Windows set `XDIALOG_TEST_NO_ACTIVATE=1` and
  `XDIALOG_TEST_POS=offscreen` so test windows never take focus.
- `cargo test --release --features _test-hooks --test offscreen` checks the deterministic
  offscreen renders against `tests/visual_references/offscreen/<renderer>/<theme>/`
  (`XDIALOG_BLESS=1` re-writes them; the `d2d` and `cg` ones apply only when this machine's
  system fonts match `tests/visual_references/offscreen/<renderer>/FONT_ID`).
- `cargo run --release --example gallery --features _test-hooks -- --theme all` renders every
  dialog variant of every look to `target/gallery/<renderer>/<theme>/` plus a contact sheet
  (`--out <dir>`, `--filter <substr>`). `--theme macos` renders both macOS styles, Sequoia into
  `macos/` and Tahoe into `macos_tahoe/`; `--theme macos_legacy` or `--theme macos_tahoe` renders
  one of them.
- `XDIALOG_VISUAL_SEED=1 cargo test --test visual_regression` re-seeds the screenshot references
  of `tests/visual_regression.rs` (Win32 TaskDialog on Windows, AppKit on macOS; it takes focus,
  `XDIALOG_VISUAL_TEST=1` compares).
- Hidden environment variables (testing only): `XDIALOG_BACKEND=auto|win32|fluent|ubuntu|macos|appkit`
  overrides the backend choice; `XDIALOG_TEST_NO_ACTIVATE`, and in debug or `_test-hooks` builds
  `XDIALOG_TEST_POS`, `XDIALOG_TEST_ACCENT`, `XDIALOG_TEST_MAC_STYLE=legacy|tahoe` (the drawn
  macOS look's style, otherwise chosen by the running macOS version), `XDIALOG_TEST_D2D_FAIL`
  (Windows: the Direct2D window surface can't be created, as without Direct2D),
  `XDIALOG_TEST_FAIL_DRAWN` (the drawn runtime fails to show any dialog) and
  `XDIALOG_TEST_STUB_TASKDIALOG` (TaskDialogs answer at once with their default button, no
  window); `tests/fallback.rs` uses the last two.
