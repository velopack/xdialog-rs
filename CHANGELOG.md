# Changelog

## 4.0.0

The Linux backend was rewritten on [egui](https://github.com/emilk/egui) 0.36 (software rendering,
no GPU), which also brings a Fluent (WinUI 3) look to Windows. The backend is now chosen at
runtime, and xdialog can run inside a winit event loop your application owns. The Linux dialogs
keep the look of 3.x: same layout, Ubuntu font, colours, metrics, animations and keyboard
behaviour.

### Breaking changes

- **Windows 10 and later now show the Fluent look by default** instead of Win32 TaskDialog
  (falling back to TaskDialog if the egui backend fails). Older Windows keep TaskDialog. Use
  `XDialogBuilder::new().with_backend(XDialogBackend::Win32)` for the previous behaviour, e.g. if
  you need screen reader support.
- **Rust 1.95** is now required on every platform (egui 0.36's MSRV): egui, winit 0.30 and
  softbuffer are always compiled.
- **One winit loop per process:** with an egui backend (the default on Windows 10+ and Linux)
  `XDialogBuilder::run*` owns a winit 0.30 event loop. An application with its own winit loop must
  use `winit-host` instead.
- **New error variant `XDialogError::BlockingCallOnUiThread`.** A blocking dialog call
  (`show_message*`) made on xdialog's UI thread, e.g. from a progress button callback of the
  egui or AppKit backends, used to deadlock; it now returns this error. (Win32 TaskDialog
  callbacks run on the dialog's own thread and are unaffected.)
- **`XDialogError` is now `#[non_exhaustive]`**, so future variants are not breaking changes.
  Exhaustive `match`es need a wildcard arm.
- Dialog calls after the backend has shut down (the builder's event loop ended, or the
  host app exited) now return `XDialogError::NoBackendAvailable` instead of
  `SendFailed`. `NoBackendAvailable` is also returned when the chosen `XDialogBackend` can't run
  here.
- Internal plumbing is no longer public: `DialogMessageRequest`, `CreationSender`,
  `ProgressButtonCallback` and `XDialogError::SendFailed` were removed from the API.
- `XDialogError::NoResult` now carries `std::sync::mpsc::RecvError` (the `oneshot` dependency was
  dropped).
- **The skia backend was removed**, together with the `skia-instrumentation` feature, the hidden
  `xdialog::pixels` module, the `skia_bench` example and the benchmarks. Dependencies dropped:
  tiny-skia, cosmic-text, enum-map, mina, multiversion, sysinfo, oneshot, widestring, block2 (and
  criterion, dev-only).

### Added

- `XDialogBackend` and `XDialogBuilder::with_backend`: choose the backend at runtime. `Auto` (the
  default) is Fluent with a Win32 TaskDialog fallback on Windows 10+, Win32 on older Windows,
  Ubuntu on Linux (`NoBackendAvailable` without a display server) and AppKit on macOS. `Fluent`
  and `Ubuntu` run wherever winit does.
- Fluent backend: WinUI 3 ContentDialog-style dialogs (Segoe UI Variable, system accent colour,
  light/dark) drawn with egui.
- `winit-host` feature: `XDialogBuilder::into_host_app(app, waker)` wraps your winit
  `ApplicationHandler` in an `xdialog::host::XDialogApp` that runs xdialog's dialogs inside the
  event loop your application owns, with no xdialog code in your handler: it handles the dialog
  windows' events, merges their wake-up deadline into your control flow and closes the dialogs on
  `exiting`; everything else is forwarded. xdialog creates its windows through your `ActiveEventLoop`;
  `xdialog::host::winit` re-exports its winit 0.30 so your loop uses the same version. On macOS
  `Auto` uses the Ubuntu look in host mode (AppKit needs its own loop). See
  `examples/winit_host.rs`.
- `XDialogTheme` is now `Copy` and `Default` (`SystemDefault`).
- Right-to-left text (Arabic, Hebrew) is reordered per line (unicode-bidi).
- System font fallback for characters the bundled font lacks, including the family's bold face
  for headings, placed on the primary font's baseline. Linux scans fonts with fontdb on a
  background thread; Windows uses a known list of system fonts ordered by the user's locale.

### Changed

- Linux: the XDG desktop portal (dark mode, accent colour) is read on a background thread, and
  changes are applied to open dialogs.
- `XDialogBuilder::run*` no longer hangs when another handler (`init_win32_direct`, `into_host_app`)
  was installed first: it runs `main` and the requests go to that handler, which it no longer
  shuts down when `main` returns. A panic in `main` no longer leaves the event loop running: the
  backend is stopped and the panic resumed.
- Progress dialogs shown from xdialog's UI thread (`show_progress*` inside an egui or AppKit
  callback) return their proxy at once; the window is created on the next loop iteration.
- macOS: `show_progress*` from an AppKit button callback no longer deadlocks (it returns its proxy
  at once), and `show_message*` there returns `BlockingCallOnUiThread` instead of deadlocking.

### Known limitations

See [Limitations of the egui backends](README.md#limitations-of-the-egui-backends) (accessibility,
emoji, complex scripts). 3.x drew colour emoji on Linux; text rendering differs slightly from 3.x
(egui's rasterizer instead of cosmic-text).

### Tests and CI

- The egui looks are pinned by deterministic offscreen goldens (`tests/egui_offscreen.rs`) instead
  of on-screen Linux screenshots; on-screen captures remain for Win32 TaskDialog and AppKit.
