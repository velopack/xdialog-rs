# xdialog
[![Version](https://img.shields.io/crates/v/xdialog?style=flat-square)](https://crates.io/crates/xdialog)
[![License](https://img.shields.io/crates/l/xdialog?style=flat-square)](https://github.com/velopack/xdialog/blob/master/LICENSE)

A cross-platform library for displaying native dialogs in Rust: message boxes and progress
dialogs from any thread, with one API on Windows, macOS and Linux. The Windows and Linux looks
are drawn with [egui](https://github.com/emilk/egui) by a pure Rust software renderer (winit +
softbuffer, no GPU and no C/C++ build dependencies, static musl compatible); Win32 TaskDialog
and AppKit are the native backends.

This is not a replacement for a proper GUI framework. It is meant to be used for CLI / background
applications which occasionally need to show dialogs (such as alerts, or progress) to the user.

It's main use-case is for the [Velopack](https://velopack.io) application installation and
update framework.

## Features
- Cross-platform: works on Windows, macOS, and Linux
- A WinUI 3 (Fluent) look on Windows 10+, the classic xdialog look on Linux, native Win32
  TaskDialog and AppKit; the backend is chosen at runtime
- Pure Rust software rendering (no GPU, no C/C++ dependencies, static musl compatible)
- Embedded font (Ubuntu) - no system font dependencies on Linux
- Runs its own event loop, or inside a winit event loop your application already runs
- Simple and consistent API across all platforms

## Installation

Add the following to your `Cargo.toml`:
```toml
[dependencies]
xdialog = "4.0.0"
```

Or, run the following command:
```sh
cargo add xdialog
```

Rust 1.95 or newer is required (egui 0.36).

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
    "Would you like to to the new version now?",
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
| Windows 10 and later | `Fluent` (the WinUI 3 look, Segoe UI Variable, system accent colour), falling back to Win32 TaskDialog if the egui backend fails (its event loop, a window or its first frame can't be created) |
| Older Windows | `Win32` TaskDialog |
| Linux | `Ubuntu` (the classic xdialog look, bundled Ubuntu font) |
| Linux, no display server | every dialog function returns `XDialogError::NoBackendAvailable`; your program keeps running |
| macOS | `AppKit` |

`Fluent` and `Ubuntu` can be chosen on any platform where winit runs; `Win32` and `AppKit` only on
their own. A backend that can't run here gives `NoBackendAvailable`; if the backend runs but a
dialog's window can't be created, that call returns `SystemError` (except where `Auto` falls back
to TaskDialog). The egui backends follow the
system light/dark preference (the Windows registry, the XDG desktop portal on Linux) unless
`XDialogBuilder::with_theme` forces one.

## Custom icons

`XDialogOptions::icon_source` takes an `.ico`, `.png` or `.icns` image, as a file
(`XDialogIconSource::File`) or its bytes (`XDialogIconSource::Bytes`); the format is read from the
content, so any of the three works on every platform. With the egui backends (Fluent, Ubuntu) it
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
| `winit-host` | `XDialogBuilder::into_host_app` and `xdialog::host` (with `xdialog::host::winit`, a re-export of xdialog's winit 0.30): run the dialogs inside a winit event loop your application owns |
| `win32-direct` | `init_win32_direct()` (Windows): Win32 TaskDialog without an `XDialogBuilder` |
| `maccf-direct` | `init_maccf_direct()` (macOS): CFUserNotification without an `XDialogBuilder` |

### Using your own winit event loop

winit allows one event loop per process, and `XDialogBuilder::run` runs one for the egui
backends. An application with its own winit loop enables `winit-host`, wraps its
`ApplicationHandler` with `XDialogBuilder::into_host_app` on its event-loop thread instead of
calling `run`, and passes the wrapper to `run_app`. The handler needs no xdialog code: the wrapper
handles the dialog windows' events, merges their wake-up deadline into your control flow and
closes the dialogs on exit. On macOS `Auto` uses the `Ubuntu` look there, since AppKit needs its
own loop. See the
[`xdialog::host`](https://docs.rs/xdialog/latest/xdialog/host/index.html) documentation and
[`examples/winit_host.rs`](https://github.com/velopack/xdialog/blob/master/examples/winit_host.rs),
a complete host.

### Threads

Dialog functions can be called from any thread. A *blocking* call (the `show_message_*`
shortcuts, `MessageDialogProxy::wait`) made on xdialog's UI thread would deadlock, so it returns
`XDialogError::BlockingCallOnUiThread` instead (only direct calls are detected: a UI thread
waiting on another thread that is inside `show_message_yes_no` still deadlocks). There, use
`show_message`: it returns a `MessageDialogProxy` at once, whose result you check with
`try_result` (xdialog wakes the loop when it arrives), `.await`, or drop to close the dialog.
`show_progress*` there returns `Ok` immediately and the window appears on the next loop
iteration; if it then can't be created, the error is only logged and the proxy does nothing. The UI thread is the egui event-loop thread (where progress button callbacks run), the
host's event-loop thread in `winit-host` mode, and on macOS the thread running the AppKit loop.
With Win32 TaskDialog every dialog runs on its own thread, so its callbacks may call any dialog
function.

## Limitations of the egui backends

- **Emoji** are drawn as monochrome outlines on Windows (Segoe UI Emoji). On Linux only outline
  emoji fonts can be used; most distributions ship only the bitmap *Noto Color Emoji*, so emoji
  show as empty boxes there. Colour emoji should return with egui 0.37.
- **Complex scripts:** right-to-left text (Arabic, Hebrew) is reordered correctly, but shaping is
  limited to what egui's text engine does. Glyphs come from the system's fonts (fontconfig
  directories on Linux, a known list of Windows fonts on Windows).

## More

See [CHANGELOG.md](https://github.com/velopack/xdialog/blob/master/CHANGELOG.md) for what changed in 4.0,
and [CONTRIBUTING.md](https://github.com/velopack/xdialog/blob/master/CONTRIBUTING.md) for how the
crate is built and tested.
