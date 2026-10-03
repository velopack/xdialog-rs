# xdialog
[![Version](https://img.shields.io/crates/v/xdialog?style=flat-square)](https://crates.io/crates/xdialog)
[![License](https://img.shields.io/crates/l/xdialog?style=flat-square)](https://github.com/velopack/xdialog/blob/master/LICENSE)

A cross-platform library for displaying native dialogs in Rust: message boxes and progress
dialogs from any thread, with one API on Windows, macOS and Linux. The Fluent (Windows), macOS
and Ubuntu (Linux) looks are drawn by xdialog itself on winit windows, through each OS's own
renderer: Direct2D and DirectWrite on Windows, CoreGraphics and CoreText on macOS, and a pure
Rust software renderer on Linux (no GPU and no C/C++ build dependencies, static musl compatible).
Win32 TaskDialog and AppKit are the native backends.

This is not a replacement for a proper GUI framework. It is meant to be used for CLI / background
applications which occasionally need to show dialogs (such as alerts, or progress) to the user.

It's main use-case is for the [Velopack](https://velopack.io) application installation and
update framework.

## Features
- Cross-platform: works on Windows, macOS, and Linux
- A WinUI 3 (Fluent) look on Windows 10+, the macOS alert look on macOS (the centred alert of
  Big Sur to Sequoia, or Tahoe's left-aligned Liquid Glass alert on macOS 26+), the classic
  xdialog look on Linux, native Win32 TaskDialog and AppKit; the backend is chosen at runtime
- Drawn with the platform's renderer and fonts on Windows and macOS; pure Rust software
  rendering on Linux (no GPU, no C/C++ dependencies, static musl compatible)
- Embedded font (Ubuntu) on Linux only - no system font dependencies there; Windows and macOS
  builds bundle no fonts
- Colour emoji and complex-script shaping on every OS
- Accessible: screen readers see and can operate the drawn dialogs (AccessKit)
- Runs its own event loop, or inside a winit event loop your application already runs
- Simple and consistent API across all platforms

## Installation

Add the following to your `Cargo.toml`:
```toml
[dependencies]
xdialog = "4.1.0"
```

Or, run the following command:
```sh
cargo add xdialog
```

Rust 1.95 or newer is required.

## Usage
Since some platforms require UI to be run on the main thread, xdialog expects to own the
main thread, and will launch your core application logic in another thread.

```rust,no_run
use xdialog::*;

fn main() {
  let code = XDialogBuilder::new().run_i32(your_main_logic);
  std::process::exit(code);
}

fn your_main_logic() -> i32 {

  // ... do something here

  let should_update_now = show_message_yes_no(
    "My App Incorporated",
    "New version available",
    "Would you like to update to the new version now?",
    XDialogIcon::Warning,
  ).unwrap();

  if !should_update_now {
    return -1; // user declined the dialog
  }

  // ... do something here

  let progress = show_progress(
    "My App Incorporated",
    "Main instruction",
    "Body text",
    XDialogIcon::Information
  ).unwrap();

  progress.set_value(0.5).unwrap();
  progress.set_text("Extracting...").unwrap();
  std::thread::sleep(std::time::Duration::from_secs(3));

  progress.set_value(1.0).unwrap();
  progress.set_text("Updating...").unwrap();
  std::thread::sleep(std::time::Duration::from_secs(3));

  progress.set_indeterminate().unwrap();
  progress.set_text("Wrapping Up...").unwrap();
  std::thread::sleep(std::time::Duration::from_secs(3));

  progress.close().unwrap();
  0 // return exit code
}
```

There are more examples in the `examples` directory.
```sh
cargo run --example various_options
```

## Backends

The backend is chosen at runtime with `XDialogBuilder::with_backend`. The default,
`XDialogBackend::Auto`, picks:

| Platform | `XDialogBackend::Auto` |
|---|---|
| Windows 10 and later | `Fluent` (the WinUI 3 look, Segoe UI Variable, system accent colour), falling back to Win32 TaskDialog if the drawn backend fails (its event loop, a window or its first frame can't be created) |
| Older Windows | `Win32` TaskDialog |
| Linux | `Ubuntu` (the classic xdialog look, bundled Ubuntu font) |
| Linux, no display server | every dialog function returns `XDialogError::NoBackendAvailable`; your program keeps running |
| macOS | `MacOS` (the alert of macOS 11 to 15: SF Pro, the translucent alert material, system accent colour and alert icons) |

`Fluent`, `Ubuntu` and `MacOS` can be chosen on any platform where winit runs; `Win32` and `AppKit` only on
their own. A backend that can't run here gives `NoBackendAvailable`; if the backend runs but a
dialog's window can't be created, that call returns `SystemError` (except where `Auto` falls back
to TaskDialog). The drawn backends follow the
system light/dark preference (the Windows registry, the XDG desktop portal on Linux, AppKit's
effective appearance on macOS) unless
`XDialogBuilder::with_theme` forces one.

Win32 TaskDialog (`XDialogBackend::Win32`, `win32-direct`, and the fallback of `Auto`) is part of
version 6 of the Windows Common Controls. Your executable needs no manifest for it: if its own
manifest doesn't select v6, xdialog activates v6 for its dialogs.

## Custom icons

`XDialogOptions::icon_source` takes an `.ico`, `.png` or `.icns` image, as a file
(`XDialogIconSource::File`) or its bytes (`XDialogIconSource::Bytes`); the format is read from the
content, so any of the three works on every platform. With the drawn backends (Fluent, Ubuntu, MacOS) it
becomes the dialog's window and taskbar icon where the platform has one (Windows, and X11 on
Linux; Wayland and macOS have no per-window icons), and with `XDialogIcon::Custom` it is shown in
the dialog instead of the information, warning or error icon. `Custom` without an icon source (or
with one that can't be loaded) shows no icon; it is never an error. Win32 TaskDialog, AppKit and
`maccf-direct` ignore the icon source (`Custom` shows no icon there).

```rust,no_run
# use xdialog::*;
let options = XDialogOptions { title: "My App".into(),
                               main_instruction: "Update available".into(),
                               message: "Version 2.0 is ready to install.".into(),
                               icon: XDialogIcon::Custom,
                               icon_source: Some(XDialogIconSource::File("assets/app.ico".into())),
                               buttons: vec!["Later".into(), "Install".into()] };
let result = show_message(options).wait();
```

## Cargo features

None are on by default.

| Feature | What it does |
|---|---|
| `winit-host` | `XDialogBuilder::into_host` / `into_host_app` and `xdialog::host` (with `xdialog::host::winit`, a re-export of xdialog's winit 0.30): run the dialogs inside a winit event loop your application owns |
| `win32-direct` | `init_win32_direct()` (Windows): Win32 TaskDialog without an `XDialogBuilder` |
| `maccf-direct` | `init_maccf_direct()` (macOS): CFUserNotification without an `XDialogBuilder` |

### Using your own winit event loop

winit allows one event loop per process, and `XDialogBuilder::run` runs one for the drawn
backends (Fluent, Ubuntu, MacOS). An application with its own winit loop enables `winit-host`, creates an
`XDialogHost` with `XDialogBuilder::into_host` on its event-loop thread instead of calling `run`,
and passes `host.wrap(&mut app)` to `run_app_on_demand` (or `pump_app_events`) for each run of
its loop; a loop that runs once with `run_app` can use `XDialogBuilder::into_host_app` instead,
which wraps the app for good. The handler needs no xdialog code: the wrapper handles the dialog
windows' events, merges their wake-up deadline into your control flow and closes the dialogs when
a run ends. Dialogs requested between runs are shown by the next run; dropping the host ends
xdialog for the process. `AppKit` is not available there, since it needs its own loop. See the
[`xdialog::host`](https://docs.rs/xdialog/latest/xdialog/host/index.html) documentation,
[`examples/winit_host.rs`](https://github.com/velopack/xdialog/blob/master/examples/winit_host.rs),
a complete host, and
[`examples/winit_host_on_demand.rs`](https://github.com/velopack/xdialog/blob/master/examples/winit_host_on_demand.rs),
which runs its loop several times.

### Threads

Dialog functions can be called from any thread. A *blocking* call (the `show_message_*`
shortcuts, `MessageDialogProxy::wait`) made on xdialog's UI thread would deadlock, so it returns
`XDialogError::BlockingCallOnUiThread` instead (only direct calls are detected: a UI thread
waiting on another thread that is inside `show_message_yes_no` still deadlocks). There, use
`show_message`: it returns a `MessageDialogProxy` at once, whose result you check with
`try_result` (xdialog wakes the loop when it arrives), `.await`, or drop to close the dialog.
`show_progress*` there returns `Ok` immediately and the window appears on the next loop
iteration; if it then can't be created, the error is only logged and the proxy does nothing. The UI thread is the event-loop thread of the drawn backends (where progress button callbacks run), the
host's event-loop thread in `winit-host` mode, and on macOS the thread running the AppKit loop.
With Win32 TaskDialog every dialog runs on its own thread, so its callbacks may call any dialog
function.

## The drawn backends

The Fluent, Ubuntu and macOS looks record their drawing into a small display list that one of three
renderers replays; exactly one is compiled for each target:

| Target | Renderer | Fonts |
|---|---|---|
| Windows | Direct2D + DirectWrite | the system's (Segoe UI Variable, Segoe UI), with DirectWrite's fallback |
| macOS | CoreGraphics + CoreText | the system's, with CoreText's fallback |
| Linux and the BSDs | software (vello_cpu + cosmic-text), presented with softbuffer | the bundled Ubuntu font, then the system's fonts (scanned on a background thread) |

- **Emoji** are drawn in colour: COLR (Segoe UI Emoji) on Windows, sbix (Apple Color Emoji) on
  macOS, and COLR or CBDT (e.g. *Noto Color Emoji*) on Linux.
- **Text** is shaped by DirectWrite, CoreText or cosmic-text (HarfRust), so complex scripts and
  right-to-left text (Arabic, Hebrew) are laid out by the platform's rules. Output is close, not
  pixel-identical, across OSes: fonts and metrics come from each platform.
- **Accessibility:** each drawn dialog is exposed through AccessKit (UI Automation on Windows,
  AT-SPI on Linux, NSAccessibility on macOS): screen readers read the title, texts, icon,
  buttons and progress, follow the keyboard focus and can press the buttons.

## More

See [CHANGELOG.md](https://github.com/velopack/xdialog/blob/master/CHANGELOG.md) for what changed in 4.0,
and [CONTRIBUTING.md](https://github.com/velopack/xdialog/blob/master/CONTRIBUTING.md) for how the
crate is built and tested.

## License

xdialog is MIT licensed. Linux and BSD builds embed the Ubuntu font (Regular and Bold), which is
under the [Ubuntu Font Licence 1.0](https://github.com/velopack/xdialog/blob/master/src/backends/draw/soft/fonts/LICENCE.txt):
a binary that ships those builds redistributes the font and must include that licence. Windows
and macOS builds embed no fonts.
