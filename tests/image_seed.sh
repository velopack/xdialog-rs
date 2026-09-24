#!/bin/bash
# Generates visual reference screenshots (tests/visual_regression.rs) for the current platform.
# Run this on each target OS to create/update the reference images.
#
# Usage:
#   ./tests/image_seed.sh                          # default features
#   ./tests/image_seed.sh --features fluent-egui   # Windows: the Fluent (egui) builder backend
#
# Reference directories (tests/visual_references/<dir>/):
#   windows         Win32 TaskDialog (default Windows build)
#   windows_fluent  Fluent egui backend (--features fluent-egui). Captured with PrintWindow, never
#                   activated (XDIALOG_TEST_NO_ACTIVATE); forced light theme and default accent.
#                   Local-only: compared only when C:\Windows\Fonts\SegUIVar.ttf matches the
#                   FONT_SHA256 recorded here while seeding (GitHub's runners don't ship it).
#   linux           X11 (Xvfb + openbox), the egui Linux backend
#   linux_wayland   Wayland (headless sway + grim)
#   macos           AppKit
#
# xdialog 4.0 replaced the skia Linux renderer with egui: the linux/ and linux_wayland/
# references must be re-seeded on Linux (e.g. from the CI "visual-regression-output-*"
# artifacts, or by running this script in the CI environment).
set -e

CARGO_ARGS="${@}"

# Detect platform
case "$(uname -s)" in
    Linux*)
        if [ -n "$WAYLAND_DISPLAY" ]; then PLATFORM="linux_wayland"; else PLATFORM="linux"; fi;;
    Darwin*)  PLATFORM="macos";;
    MINGW*|MSYS*|CYGWIN*)
        case " $CARGO_ARGS " in
            *fluent-egui*|*all-features*) PLATFORM="windows_fluent";;
            *) PLATFORM="windows";;
        esac;;
    *) echo "Unsupported platform: $(uname -s)"; exit 1;;
esac

echo "Generating visual references for platform: $PLATFORM"

# Linux: start virtual display if none available
if [ "$PLATFORM" = "linux" ] && [ -z "$DISPLAY" ]; then
    echo "No DISPLAY set, starting Xvfb..."
    Xvfb :99 -screen 0 1280x1024x24 &
    XVFB_PID=$!
    export DISPLAY=:99
    trap "kill $XVFB_PID 2>/dev/null" EXIT
    sleep 1
fi

# Run visual tests in seed mode (one process; the dialogs are shown one after another)
XDIALOG_VISUAL_SEED=1 cargo test $CARGO_ARGS --test visual_regression

echo ""
echo "Reference images saved to tests/visual_references/$PLATFORM/"
ls -la "tests/visual_references/$PLATFORM/"
