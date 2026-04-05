# xdialog
[![Version](https://img.shields.io/crates/v/xdialog?style=flat-square)](https://crates.io/crates/xdialog)
[![License](https://img.shields.io/crates/l/xdialog?style=flat-square)](https://github.com/velopack/xdialog/blob/master/LICENSE)

A cross-platform library for displaying native dialogs in Rust. On Windows and macOS, this
library uses native system dialogs (Win32 TaskDialog and AppKit). On Linux, it uses FLTK by
default, with optional GTK3 support. This allows for a simplified API and consistent behavior
across platforms.

This is not a replacement for a proper GUI framework. It is meant to be used for CLI / background
applications which occasionally need to show dialogs (such as alerts, or progress) to the user.

It's main use-case is for the [Velopack](https://velopack.io) application installation and
update framework.

## Features
- Cross-platform: works on Windows, macOS, and Linux
- Native backends on Windows (Win32) and macOS (AppKit) with zero additional build dependencies
- FLTK backend on Linux with optional GTK3 support
- Simple and consistent API across all platforms

## Installation

Add the following to your `Cargo.toml`:
```toml
[dependencies]
xdialog = "0" # replace with the latest version
```

Or, run the following command:
```sh
cargo install xdialog
```

## Usage
Since some platforms require UI to be run on the main thread, xdialog expects to own the
main thread, and will launch your core application logic in another thread.

```rust
use xdialog::*;

fn main() -> i32 {
  XDialogBuilder::new().run(your_main_logic)
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
- **Windows**: Native Win32 TaskDialog API
- **macOS**: Native AppKit dialogs
- **Linux**: [fltk-rs](https://github.com/fltk-rs/fltk-rs) by default, with optional GTK3 support via the `gtk` feature

On Linux, pre-compiled FLTK binaries are bundled for common architectures (x64, arm64).
To enable GTK3 support, add the `gtk` feature to your dependency and install GTK3 development
libraries (`libgtk-3-dev` on Debian/Ubuntu). When the `gtk` feature is enabled, GTK3 becomes
the default backend, with automatic fallback to FLTK if GTK fails to initialize.

```toml
[dependencies]
xdialog = { version = "0", features = ["gtk"] }
```
