use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use image::RgbaImage;
use xdialog::*;

// --- Constants ---

/// Time to wait for a dialog to fully render before first capture attempt.
const RENDER_WAIT_MS: u64 = 1500;

/// Timeout for message dialogs (must be longer than RENDER_WAIT_MS + retry window).
const DIALOG_TIMEOUT_SECS: u64 = 15;

/// Per-channel pixel difference threshold (0-255). Differences at or below this
/// are considered identical (handles anti-aliasing variations).
const PIXEL_THRESHOLD: u8 = 10;

/// Maximum fraction of pixels that can differ before the test fails.
const DIFF_PERCENT_THRESHOLD: f64 = 0.05; // 5%

// --- Platform-specific window capture ---
// Each module exposes `try_capture(title)`: one attempt at capturing the window with that exact
// title (`None`: not found yet or the capture failed).

/// Win32 TaskDialog: screen-DC capture of the whole window.
#[cfg(windows)]
mod capture {
    use super::*;
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// Captures a window by exact title using the screen DC (works with DWM compositing).
    pub fn try_capture(title: &str) -> Option<RgbaImage> {
        let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();

        unsafe {
            let hwnd = FindWindowW(None, windows::core::PCWSTR(title_wide.as_ptr())).ok()?;
            if hwnd.is_invalid() {
                return None;
            }

            // Unfocus the dialog by focusing the desktop window, so the
            // titlebar is always rendered in its inactive state. This makes
            // captures consistent across focused/unfocused environments (e.g. CI).
            let desktop = GetDesktopWindow();
            let _ = SetForegroundWindow(desktop);
            thread::sleep(Duration::from_millis(100));

            let mut rect = RECT::default();
            GetWindowRect(hwnd, &mut rect).ok()?;
            let width = (rect.right - rect.left) as u32;
            let height = (rect.bottom - rect.top) as u32;
            if width == 0 || height == 0 {
                eprintln!("Window found but has zero size: {}x{}", width, height);
                return None;
            }

            // Capture from the screen DC at the window's position
            // This correctly captures DWM-composited content
            let hdc_screen = GetDC(None);
            let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
            let hbitmap = CreateCompatibleBitmap(hdc_screen, width as i32, height as i32);
            let old_bitmap = SelectObject(hdc_mem, hbitmap.into());

            let _ = BitBlt(
                hdc_mem, 0, 0, width as i32, height as i32,
                Some(hdc_screen), rect.left, rect.top, SRCCOPY,
            );

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    biHeight: -(height as i32), // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: 0, // BI_RGB
                    ..Default::default()
                },
                ..Default::default()
            };

            let mut pixels = vec![0u8; (width * height * 4) as usize];
            GetDIBits(
                hdc_mem,
                hbitmap,
                0,
                height,
                Some(pixels.as_mut_ptr() as *mut _),
                &mut bmi,
                DIB_RGB_COLORS,
            );

            // Cleanup GDI objects
            SelectObject(hdc_mem, old_bitmap);
            let _ = DeleteObject(hbitmap.into());
            let _ = DeleteDC(hdc_mem);
            ReleaseDC(None, hdc_screen);

            // BGRA -> RGBA
            for chunk in pixels.as_chunks_mut::<4>().0 {
                chunk.swap(0, 2);
            }

            RgbaImage::from_raw(width, height, pixels)
        }
    }
}

#[cfg(target_os = "linux")]
mod capture {
    use super::*;

    /// X11: the window by title. Wayland: the whole compositor output (no per-window capture).
    pub fn try_capture(title: &str) -> Option<RgbaImage> {
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            try_capture_wayland()
        } else {
            try_capture_x11(title)
        }
    }

    fn try_capture_x11(title: &str) -> Option<RgbaImage> {
        static LISTED: std::sync::Once = std::sync::Once::new();
        let windows = match xcap::Window::all() {
            Ok(w) => w,
            Err(e) => {
                eprintln!("xcap Window::all() failed: {}", e);
                return None;
            }
        };
        LISTED.call_once(|| {
            let titles: Vec<_> = windows.iter().filter_map(|w| w.title().ok()).collect();
            eprintln!("xcap found {} windows: {:?}", titles.len(), titles);
        });
        let window = windows.iter().find(|w| w.title().unwrap_or_default() == title)?;
        match window.capture_image() {
            Ok(img) if img.width() > 0 && img.height() > 0 => Some(img),
            Ok(img) => {
                eprintln!("Window found but has zero size: {}x{}", img.width(), img.height());
                None
            }
            Err(e) => {
                eprintln!("xcap capture_image() failed: {}", e);
                None
            }
        }
    }

    /// Captures the entire compositor output via `grim` (needs wlr-screencopy).
    fn try_capture_wayland() -> Option<RgbaImage> {
        let tmp = std::env::temp_dir().join("xdialog_wayland_capture.png");
        let output = std::process::Command::new("grim").arg(&tmp).output().ok()?;
        if !output.status.success() {
            eprintln!("grim failed: {}", String::from_utf8_lossy(&output.stderr));
            return None;
        }
        let img = image::open(&tmp).ok()?.to_rgba8();
        std::fs::remove_file(&tmp).ok();
        (img.width() > 0 && img.height() > 0).then_some(img)
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod capture {
    use super::*;

    /// Resign our app's active status (by activating Finder) so the dialog is captured in its
    /// inactive/unfocused state. This mirrors the Windows capture path, which focuses the desktop
    /// window. Without it, whether the dialog is "key" is nondeterministic on a CI session, so the
    /// default-button highlight (blue vs grey) and titlebar shade flip between runs and the diff
    /// drifts across the threshold. References are seeded in the unfocused state.
    #[cfg(target_os = "macos")]
    pub fn before_capture() {
        let _ = std::process::Command::new("osascript").args(["-e", "tell application \"Finder\" to activate"]).status();
        thread::sleep(Duration::from_millis(250));
    }

    #[cfg(target_os = "macos")]
    pub fn try_capture(title: &str) -> Option<RgbaImage> {
        let windows = xcap::Window::all().map_err(|e| eprintln!("Failed to list windows: {}", e)).ok()?;
        let window = windows.iter().find(|w| w.title().ok().as_deref() == Some(title))?;
        window.capture_image().map_err(|e| eprintln!("Failed to capture window: {}", e)).ok()
    }

    #[cfg(not(target_os = "macos"))]
    pub fn try_capture(_title: &str) -> Option<RgbaImage> {
        eprintln!("Unsupported platform for window capture");
        None
    }
}

/// Finds a window by exact title and captures it to a PNG file.
/// Retries several times to handle timing where the dialog hasn't appeared yet.
fn capture_window_to_file(title: &str, output_path: &Path) -> bool {
    const MAX_ATTEMPTS: u32 = 20;
    const RETRY_DELAY_MS: u64 = 500;

    #[cfg(target_os = "macos")]
    capture::before_capture();

    for attempt in 1..=MAX_ATTEMPTS {
        if let Some(img) = capture::try_capture(title) {
            if let Some(parent) = output_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            return match img.save(output_path) {
                Ok(_) => {
                    eprintln!("Captured '{}' ({}x{}) on attempt {}", title, img.width(), img.height(), attempt);
                    true
                }
                Err(e) => {
                    eprintln!("Failed to save screenshot: {}", e);
                    false
                }
            };
        }
        if attempt == 1 {
            eprintln!("Window '{}' not found yet, retrying...", title);
        }
        if attempt < MAX_ATTEMPTS {
            thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
        }
    }
    eprintln!("Window '{}' not found after {} attempts ({:.1}s)", title, MAX_ATTEMPTS, MAX_ATTEMPTS as f64 * RETRY_DELAY_MS as f64 / 1000.0);
    false
}

// --- Helpers ---

fn platform_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            "linux_wayland"
        } else {
            "linux"
        }
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        panic!("unsupported platform for visual tests")
    }
}

fn is_seed_mode() -> bool {
    std::env::var("XDIALOG_VISUAL_SEED").is_ok()
}

fn reference_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("visual_references")
        .join(platform_name())
}

fn output_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("visual_output")
}

/// Pixels to ignore around the edge of the window. The screen-DC capture
/// includes the DWM shadow / background behind the window which can vary.
const EDGE_MARGIN: u32 = 16;

/// Compares two images pixel-by-pixel, ignoring the outer EDGE_MARGIN pixels.
/// Returns (passed, diff_percentage). On failure, saves a diff image next to the actual image.
fn compare_images(actual_path: &Path, reference_path: &Path) -> (bool, f64) {
    let actual = image::open(actual_path)
        .expect("Failed to open actual screenshot")
        .to_rgba8();
    let reference = image::open(reference_path)
        .expect("Failed to open reference screenshot")
        .to_rgba8();

    if actual.dimensions() != reference.dimensions() {
        eprintln!(
            "Dimension mismatch: actual={}x{}, reference={}x{}",
            actual.width(),
            actual.height(),
            reference.width(),
            reference.height()
        );
        return (false, 1.0);
    }

    let w = actual.width();
    let h = actual.height();
    let x_start = EDGE_MARGIN.min(w / 2);
    let y_start = EDGE_MARGIN.min(h / 2);
    let x_end = w.saturating_sub(EDGE_MARGIN);
    let y_end = h.saturating_sub(EDGE_MARGIN);
    let compared_pixels = (x_end - x_start) as f64 * (y_end - y_start) as f64;

    let mut diff_count = 0u64;
    let mut diff_image = RgbaImage::new(w, h);

    for (x, y, actual_pixel) in actual.enumerate_pixels() {
        let ref_pixel = reference.get_pixel(x, y);

        // Skip edge margin - mark as grey in diff image
        if x < x_start || x >= x_end || y < y_start || y >= y_end {
            diff_image.put_pixel(x, y, image::Rgba([80, 80, 80, 255]));
            continue;
        }

        let max_diff = (0..3)
            .map(|i| (actual_pixel[i] as i16 - ref_pixel[i] as i16).unsigned_abs() as u8)
            .max()
            .unwrap();

        if max_diff > PIXEL_THRESHOLD {
            diff_count += 1;
            diff_image.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
        } else {
            diff_image.put_pixel(
                x,
                y,
                image::Rgba([
                    actual_pixel[0] / 3,
                    actual_pixel[1] / 3,
                    actual_pixel[2] / 3,
                    255,
                ]),
            );
        }
    }

    let diff_percent = diff_count as f64 / compared_pixels;
    let passed = diff_percent < DIFF_PERCENT_THRESHOLD;

    if !passed {
        let diff_path = actual_path.with_extension("diff.png");
        if let Err(e) = diff_image.save(&diff_path) {
            eprintln!("Failed to save diff image: {}", e);
        } else {
            eprintln!("Diff image saved to: {}", diff_path.display());
        }
    }

    (passed, diff_percent)
}

/// Either seeds the reference image or compares the captured output against it.
fn seed_or_compare(name: &str) {
    let output_path = output_dir().join(format!("{}.png", name));
    assert!(
        output_path.exists(),
        "Captured screenshot not found at {}",
        output_path.display()
    );

    if is_seed_mode() {
        let ref_dir = reference_dir();
        std::fs::create_dir_all(&ref_dir).unwrap();
        let ref_path = ref_dir.join(format!("{}.png", name));
        std::fs::copy(&output_path, &ref_path).unwrap();
        eprintln!("Seeded reference: {}", ref_path.display());
    } else {
        let ref_path = reference_dir().join(format!("{}.png", name));
        assert!(
            ref_path.exists(),
            "No reference image at {}. Run with XDIALOG_VISUAL_SEED=1 to generate references.",
            ref_path.display()
        );

        let (passed, diff_percent) = compare_images(&output_path, &ref_path);
        assert!(
            passed,
            "Visual regression failed for '{}': {:.2}% of pixels differ (threshold: {:.2}%). \
             Check tests/visual_output/{}.png and tests/visual_output/{}.diff.png",
            name,
            diff_percent * 100.0,
            DIFF_PERCENT_THRESHOLD * 100.0,
            name,
            name,
        );
    }
}

// --- Test callback (fn() for XDialogBuilder::run) ---
// Note: XDialogBuilder uses a OnceLock channel, so run() can only be called
// once per process. All visual captures must happen in a single run() call.

const DIALOG_TITLE: &str = "XDialog Visual Test";

fn run_all_captures() {
    std::fs::create_dir_all(output_dir()).ok();

    // 1. Message dialog (blocking - use timeout + capture thread)
    let output = output_dir().join("message_info.png");
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(RENDER_WAIT_MS));
        capture_window_to_file(DIALOG_TITLE, &output);
    });

    let _ = show_message(
        XDialogOptions {
            title: DIALOG_TITLE.to_string(),
            main_instruction: "Information".to_string(),
            message: "This is a test message for visual regression testing.".to_string(),
            icon: XDialogIcon::Information,
            buttons: vec!["OK".to_string()],
        },
        Some(Duration::from_secs(DIALOG_TIMEOUT_SECS)),
    );
    handle.join().unwrap();

    // 2. Progress dialog (non-blocking - capture inline)
    let output = output_dir().join("progress_0.png");
    let progress = show_progress(
        DIALOG_TITLE,
        "Working...",
        "Downloading update...",
        XDialogIcon::Information,
    )
    .unwrap();

    progress.set_value(0.0).unwrap();
    thread::sleep(Duration::from_millis(RENDER_WAIT_MS));
    capture_window_to_file(DIALOG_TITLE, &output);
    progress.close().unwrap();
}

// --- Test ---

fn main() {
    if std::env::var("XDIALOG_VISUAL_TEST").is_err() && std::env::var("XDIALOG_VISUAL_SEED").is_err() {
        eprintln!("Skipping visual regression (set XDIALOG_VISUAL_TEST=1 or XDIALOG_VISUAL_SEED=1 to run)");
        return;
    }

    // The references are Win32 TaskDialog, also when the Fluent backend is compiled in.
    #[cfg(windows)]
    std::env::set_var("XDIALOG_BACKEND", "win32");

    XDialogBuilder::new().run(run_all_captures);
    seed_or_compare("message_info");
    seed_or_compare("progress_0");
}
