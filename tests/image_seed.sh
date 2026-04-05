#!/bin/bash
# Generates visual reference screenshots for the current platform.
# Run this on each target OS to create/update the reference images.
#
# Usage:
#   ./image_seed.sh                    # Use default features
#   ./image_seed.sh --no-default-features  # Win32-only (no fltk/gtk)
#
set -e

CARGO_ARGS="${@}"

# Detect platform
case "$(uname -s)" in
    Linux*)   PLATFORM="linux";;
    Darwin*)  PLATFORM="macos";;
    MINGW*|MSYS*|CYGWIN*) PLATFORM="windows";;
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

# Run visual tests in seed mode (single-threaded to avoid dialog overlap)
XDIALOG_VISUAL_SEED=1 cargo test $CARGO_ARGS --test visual_regression -- --ignored --test-threads=1

echo ""
echo "Reference images saved to tests/visual_references/$PLATFORM/"
ls -la "tests/visual_references/$PLATFORM/"
