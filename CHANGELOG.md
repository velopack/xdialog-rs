# Changelog

## 4.0.0

The Linux backend was rewritten on winit with xdialog's own small drawing layer, which also
brings a Fluent (WinUI 3) look to Windows. Each OS draws through its own renderer: Direct2D and
DirectWrite on Windows, CoreGraphics and CoreText on macOS, and a pure Rust software renderer
(vello_cpu and cosmic-text, no GPU) on Linux and the BSDs. The backend is now chosen at runtime,
and xdialog can run inside a winit event loop your application owns. The Linux dialogs keep the
look of 3.x: same layout, Ubuntu font, colours, metrics, animations and keyboard behaviour.

### Breaking changes

- **macOS now shows the drawn `MacOS` look by default** instead of AppKit: the alert of macOS 11
  (Big Sur) to 15 (Sequoia), with the system font, the translucent alert material, the system
  accent colour and the system's alert icons. On macOS 26 (Tahoe) and later it draws Tahoe's
  alert instead: left-aligned, capsule buttons, rounder corners and the Liquid Glass material.
  Both styles show an indeterminate progress bar as a capsule sliding side to side. Use
  `XDialogBuilder::new().with_backend(XDialogBackend::AppKit)` for the previous behaviour.

- **Windows 10 and later now show the Fluent look by default** instead of Win32 TaskDialog
  (falling back to TaskDialog if the drawn backend fails). Older Windows keep TaskDialog. Use
  `XDialogBuilder::new().with_backend(XDialogBackend::Win32)` for the previous behaviour.
- **Rust 1.95** is now required on every platform. winit 0.30 is always compiled, softbuffer on
  Linux and macOS.
- **One winit loop per process:** with a drawn backend (Fluent, Ubuntu or MacOS; the default everywhere but older Windows)
  `XDialogBuilder::run*` owns a winit 0.30 event loop. An application with its own winit loop must
  use `winit-host` instead.
- **New error variant `XDialogError::BlockingCallOnUiThread`.** A blocking dialog call
  (`show_message*`) made on xdialog's UI thread, e.g. from a progress button callback of the
  drawn or AppKit backends, used to deadlock; it now returns this error. (Win32 TaskDialog
  callbacks run on the dialog's own thread and are unaffected.)
- **`show_message` no longer blocks.** It returns a `MessageDialogProxy` at once:
  `wait()` blocks for the result, `wait_timeout(d)` replaces the `timeout` parameter (it closes
  the dialog and returns `TimeoutElapsed`), `try_result()` checks without blocking, the proxy is a
  `Future`, and dropping it closes the dialog. It works on xdialog's UI thread, e.g. a
  `winit-host` app's event-loop thread. Errors (no backend, the dialog couldn't be created) are
  the proxy's result. Replace `show_message(options, None)` with `show_message(options).wait()`.
  The `show_message_*` shortcuts still block. With `maccf-direct`, a message box that can't be
  created now returns `SystemError` instead of `WindowClosed`.
- **`XDialogOptions` has a new field, `icon_source`, and `XDialogIcon` a new variant, `Custom`**
  (see Added). Struct literals need `icon_source: None` (or `..Default::default()`), exhaustive
  `match`es on `XDialogIcon` a `Custom` arm.
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
  tiny-skia, enum-map, mina, multiversion, sysinfo, oneshot, widestring, block2 (and
  criterion, dev-only). cosmic-text is now used on Linux and the BSDs only.

### Added

- `XDialogBackend` and `XDialogBuilder::with_backend`: choose the backend at runtime. `Auto` (the
  default) is Fluent with a Win32 TaskDialog fallback on Windows 10+, Win32 on older Windows,
  Ubuntu on Linux (`NoBackendAvailable` without a display server) and MacOS on macOS. `Fluent`,
  `Ubuntu` and `MacOS` run wherever winit does.
- Fluent backend: WinUI 3 ContentDialog-style dialogs (Segoe UI Variable, system accent colour,
  light/dark), drawn with Direct2D and DirectWrite on Windows. The keyboard focus visual appears
  only after keyboard navigation, not when the dialog opens.
- MacOS backend: the macOS 11-15 alert (SF Pro, centred icon and text, accent default button,
  stacked buttons when they don't fit side by side), drawn with CoreGraphics and CoreText over the
  translucent alert material, with the system's alert icons and accent colour. Return activates
  the default button, Space the focused one.
- `winit-host` feature: `XDialogBuilder::into_host_app(app, waker)` wraps your winit
  `ApplicationHandler` in an `xdialog::host::XDialogApp` that runs xdialog's dialogs inside the
  event loop your application owns, with no xdialog code in your handler: it handles the dialog
  windows' events, merges their wake-up deadline into your control flow and closes the dialogs on
  `exiting`; everything else is forwarded. xdialog creates its windows through your `ActiveEventLoop`;
  `xdialog::host::winit` re-exports its winit 0.30 so your loop uses the same version. `AppKit` is
  not available in host mode (it needs its own loop). See
  `examples/winit_host.rs`.
- `XDialogOptions::icon_source` (`XDialogIconSource::File` or `::Bytes`: an `.ico`, `.png` or
  `.icns` image) and `XDialogIcon::Custom`, for the drawn backends (Fluent, Ubuntu, MacOS): the image is
  the dialog's window and taskbar icon (Windows, X11; not Wayland or macOS), and `Custom` shows it
  in the dialog in place of the severity icon. `Custom` without a usable image shows no icon; an
  image that can't be read or decoded is logged and ignored. Win32 TaskDialog, AppKit and
  `maccf-direct` ignore the icon source and show no icon for `Custom`. Each use (title bar,
  taskbar, dialog) gets the icon file's frame nearest its size. Adds the `ico` and `icns`
  dependencies.
- `XDialogTheme` is now `Copy` and `Default` (`SystemDefault`).
- `XDialogError` is now `Clone`.
- Text is shaped by the platform (DirectWrite, CoreText, or cosmic-text with HarfRust on Linux):
  complex scripts and right-to-left text (Arabic, Hebrew) are laid out correctly, each paragraph
  aligned by its own direction.
- Colour emoji on every OS: COLR on Windows, sbix on macOS, COLR and CBDT (e.g. Noto Color Emoji)
  on Linux.
- System font fallback for characters the theme's font lacks. Windows and macOS use the system's
  fonts and bundle none; Linux bundles the Ubuntu font and scans the system's fonts on a
  background thread.
- Accessibility for the drawn backends (AccessKit): screen readers see each dialog (title, heading
  and body), its icon, texts, buttons and progress bar (value in percent), follow the keyboard
  focus and can press the buttons (UI Automation on Windows, AT-SPI on Linux, NSAccessibility on
  macOS). Always on; the tree is built on demand, when an assistive technology asks for it.

### Changed

- Linux: the XDG desktop portal (dark mode, accent colour) is read on a background thread, and
  changes are applied to open dialogs.
- `XDialogBuilder::run*` no longer hangs when another handler (`init_win32_direct`, `into_host_app`)
  was installed first: it runs `main` and the requests go to that handler, which it no longer
  shuts down when `main` returns. A panic in `main` no longer leaves the event loop running: the
  backend is stopped and the panic resumed.
- Progress dialogs shown from xdialog's UI thread (`show_progress*` inside a drawn-backend or
  AppKit callback) return their proxy at once; the window is created on the next loop iteration.
- macOS: `show_progress*` from an AppKit button callback no longer deadlocks (it returns its proxy
  at once), and `show_message*` there returns `BlockingCallOnUiThread` instead of deadlocking.

### Known limitations

The drawn looks are close, not pixel-identical, across OSes: fonts and text metrics come from
each platform's text engine, so dialog sizes can differ by a few pixels. See
[The drawn backends](README.md#the-drawn-backends). Text rendering on Linux differs slightly from
3.x (vello_cpu instead of tiny-skia).

### Tests and CI

- The drawn looks are pinned by deterministic offscreen goldens (`tests/offscreen.rs`), stored per
  renderer under `tests/visual_references/offscreen/<renderer>/`. The software renderer's goldens
  are always compared; the Direct2D and CoreGraphics ones only on a machine whose fonts and OS
  version match the recorded `FONT_ID`. On-screen captures remain for Win32 TaskDialog, AppKit and
  the Linux default (X11 and Wayland).
- CI checks that Windows and macOS builds never compile the software renderer, its text stack or
  the bundled fonts.
