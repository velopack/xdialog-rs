# Changelog

## 4.0.0

The Linux backend was rewritten on [egui](https://github.com/emilk/egui) 0.36 (software rendering,
no GPU), which also brings an optional Fluent (WinUI 3) look on Windows and two new ways to run
xdialog without `XDialogBuilder`. The Linux dialogs keep the look of 3.x: same layout, Ubuntu
font, colours, metrics, animations and keyboard behaviour.

### Breaking changes

- **Rust 1.95** is now required on Linux, and on Windows with any egui feature (egui 0.36's
  MSRV). Windows and macOS builds with default features are unaffected.
- **New default feature `builtin-winit`.** It lets xdialog own a winit 0.30 event loop (the Linux
  `XDialogBuilder` backend, `linux-direct`, `fluent-egui`). If you build with
  `default-features = false` on Linux, add `builtin-winit` (or `linux-direct`/`fluent-egui`,
  which imply it) to keep the builder backend; without it `XDialogBuilder` has no backend on
  Linux and dialog functions return `XDialogError::NoBackendAvailable`. `default-features = false`
  plus `winit-host` is the new way to build xdialog with no winit at all. Cargo features can't
  be per-target, so with default features Windows builds also compile winit 0.30 (nothing uses or
  links it unless `fluent-egui` / `linux-egui` is on); `default-features = false` avoids that.
- **New error variant `XDialogError::BlockingCallOnUiThread`.** A blocking dialog call
  (`show_message*`) made on xdialog's UI thread, e.g. from a progress button callback of the
  egui or AppKit backends, used to deadlock; it now returns this error. (Win32 TaskDialog
  callbacks run on the dialog's own thread and are unaffected.)
- **`XDialogError` is now `#[non_exhaustive]`**, so future variants are not breaking changes.
  Exhaustive `match`es need a wildcard arm.
- `XDialogError::NoBackendAvailable` now reads "no xdialog backend available (no display server,
  backend not compiled in, or host mode shut down)": it is also returned without `builtin-winit`
  on Linux, by the macOS `init_winit_host` stub and after `xdialog::host::shutdown`.
- **The skia backend was removed**, together with the `skia-instrumentation` feature, the hidden
  `xdialog::pixels` module, the `skia_bench` example and `benches/convert.rs`. Dependencies
  dropped: tiny-skia, cosmic-text, enum-map, mina, multiversion, sysinfo.

### Added

- `fluent-egui` feature: on Windows, `XDialogBuilder` shows WinUI 3 ContentDialog-style dialogs
  (Segoe UI Variable, system accent colour, light/dark) drawn with egui instead of Win32
  TaskDialog. It is a graph-wide switch: any crate in the dependency graph enabling it changes the
  Windows builder backend of the whole binary. If the egui event loop can't start, xdialog falls
  back to TaskDialog.
- `linux-direct` feature and `init_linux_direct(theme)`: show dialogs without `XDialogBuilder`.
  xdialog starts a persistent UI thread with its own winit 0.30 event loop on the first dialog
  request. Works on Linux and Windows.
- `winit-host` feature, the `xdialog::host` module and `init_winit_host(theme, waker)`: run
  xdialog's dialogs inside your application's own event loop (winit 0.29, 0.30, 0.31 or any
  toolkit with raw-window-handle 0.6 handles). xdialog creates windows through your
  `HostWindows` implementation (including `set_dark` for the title bar) and never depends on
  winit in this mode. On macOS the module is a stub. See `examples/winit_host`.
- `linux-egui` feature: compiles the Linux backend on Windows for development and testing.
- Right-to-left text (Arabic, Hebrew) is reordered per line (unicode-bidi).
- System font fallback for characters the bundled font lacks, including the family's bold face
  for headings, placed on the primary font's baseline. Linux scans fonts with fontdb on a
  background thread; Windows uses a known list of system fonts ordered by the user's locale.

### Changed

- Linux: the XDG desktop portal (dark mode, accent colour) is read on a background thread, and
  changes are applied to open dialogs.
- `XDialogBuilder::run*` no longer hangs when another handler (`init_win32_direct`,
  `init_linux_direct`, `init_winit_host`) was installed first: it runs `main` and the requests go
  to that handler.
- Progress dialogs shown from xdialog's UI thread (`show_progress*` inside an egui or AppKit
  callback) return their proxy at once; the window is created on the next loop iteration.
- macOS: `show_progress*` from an AppKit button callback no longer deadlocks (it returns its proxy
  at once), and `show_message*` there returns `BlockingCallOnUiThread` instead of deadlocking.
- `XDialogBuilder::run*` with another handler installed no longer sends that handler
  `ExitEventLoop` when `main` returns (it used to close the handler's open dialogs, and started
  linux-direct's UI thread just to do so).

### Known limitations

- **Emoji:** monochrome outlines on Windows (Segoe UI Emoji). On Linux only outline emoji fonts
  can be used; most distributions ship only the bitmap *Noto Color Emoji*, so emoji render as
  empty boxes. 3.x drew colour emoji on Linux. Colour emoji should return with egui 0.37.
- **Accessibility:** the egui backends don't expose anything to screen readers yet (3.x's Linux
  backend didn't either). Win32 TaskDialog (the Windows default) and AppKit are unaffected.
- **One winit loop per process:** applications with their own winit event loop must use
  `winit-host` instead of `linux-direct` / the Linux builder backend.
- `winit-host` on Windows shows the Linux look; use `win32-direct` for native Windows dialogs.
- Text rendering differs slightly from 3.x (egui's rasterizer instead of cosmic-text).

### Tests and CI

- New offscreen goldens for both egui looks (`tests/egui_offscreen.rs`,
  `tests/visual_references/egui/`); the Fluent ones are local-only (they depend on the installed
  Segoe UI Variable, identified by `FONT_ID`).
- **The `linux` and `linux_wayland` screenshot references still come from the removed skia
  renderer and must be re-seeded on Linux** (`tests/image_seed.sh`, or the CI failure artifact).
  Until then the Linux visual-regression CI jobs are allowed to fail.
- CI: feature matrix (`fluent-egui`, `linux-direct`, `--no-default-features --features
  winit-host`) on Windows, Linux and macOS, the `examples/winit_host` package (winit 0.29; its
  `cargo tree` must not contain winit 0.30), and an egui render benchmark replacing the skia one.
