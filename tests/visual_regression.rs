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
const DIFF_PERCENT_THRESHOLD: f64 = 0.02; // 2%

// --- Platform-specific window capture ---

#[cfg(windows)]
mod capture {
    use super::*;
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// Captures a window by exact title using the screen DC (works with DWM compositing).
    fn try_capture(title: &str) -> Option<RgbaImage> {
        let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();

        unsafe {
            let hwnd = FindWindowW(None, windows::core::PCWSTR(title_wide.as_ptr())).ok()?;
            if hwnd.is_invalid() {
                return None;
            }

            // Bring window to foreground so it's not obscured
            let _ = SetForegroundWindow(hwnd);
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
            for chunk in pixels.chunks_exact_mut(4) {
                chunk.swap(0, 2);
            }

            RgbaImage::from_raw(width, height, pixels)
        }
    }

    /// Finds a window by exact title and captures it to a PNG file.
    /// Retries several times to handle timing where the dialog hasn't appeared yet.
    pub fn capture_window_to_file(title: &str, output_path: &Path) -> bool {
        const MAX_ATTEMPTS: u32 = 20;
        const RETRY_DELAY_MS: u64 = 500;

        for attempt in 1..=MAX_ATTEMPTS {
            match try_capture(title) {
                Some(img) => {
                    if let Some(parent) = output_path.parent() {
                        std::fs::create_dir_all(parent).ok();
                    }
                    match img.save(output_path) {
                        Ok(_) => {
                            eprintln!(
                                "Captured '{}' ({}x{}) on attempt {}",
                                title,
                                img.width(),
                                img.height(),
                                attempt
                            );
                            return true;
                        }
                        Err(e) => {
                            eprintln!("Failed to save screenshot: {}", e);
                            return false;
                        }
                    }
                }
                None => {
                    if attempt < MAX_ATTEMPTS {
                        if attempt == 1 {
                            eprintln!("Window '{}' not found yet, retrying...", title);
                        }
                        thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
                    } else {
                        eprintln!(
                            "Window '{}' not found after {} attempts ({:.1}s)",
                            title,
                            MAX_ATTEMPTS,
                            MAX_ATTEMPTS as f64 * RETRY_DELAY_MS as f64 / 1000.0
                        );
                    }
                }
            }
        }

        false
    }
}

#[cfg(not(windows))]
mod capture {
    use super::*;

    pub fn capture_window_to_file(title: &str, output_path: &Path) -> bool {
        // Linux: use xdotool + import (ImageMagick)
        #[cfg(target_os = "linux")]
        {
            let wid_output = std::process::Command::new("xdotool")
                .args(["search", "--name", title])
                .output();

            let wid_str = match wid_output {
                Ok(output) => {
                    String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                }
                Err(e) => {
                    eprintln!("xdotool failed: {}. Install with: sudo apt-get install xdotool", e);
                    return false;
                }
            };

            if wid_str.is_empty() {
                eprintln!("Window '{}' not found via xdotool", title);
                return false;
            }

            if let Some(parent) = output_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }

            let status = std::process::Command::new("import")
                .args(["-window", &wid_str, output_path.to_str().unwrap()])
                .status();

            match status {
                Ok(s) if s.success() => true,
                Ok(s) => {
                    eprintln!("import exited with: {}", s);
                    false
                }
                Err(e) => {
                    eprintln!("import failed: {}. Install with: sudo apt-get install imagemagick", e);
                    false
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            eprintln!("macOS window capture not yet implemented");
            false
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            eprintln!("Unsupported platform for window capture");
            false
        }
    }
}

// --- Helpers ---

fn platform_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
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
        if !ref_path.exists() {
            eprintln!(
                "No reference image at {}, skipping comparison. Run image_seed.sh to generate references.",
                ref_path.display()
            );
            return;
        }

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
        capture::capture_window_to_file(DIALOG_TITLE, &output);
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
    let output = output_dir().join("progress_50.png");
    let progress = show_progress(
        DIALOG_TITLE,
        "Working...",
        "50% complete",
        XDialogIcon::Information,
    )
    .unwrap();

    progress.set_value(0.5).unwrap();
    thread::sleep(Duration::from_millis(RENDER_WAIT_MS));
    capture::capture_window_to_file(DIALOG_TITLE, &output);
    progress.close().unwrap();
}

// --- Test ---

#[test]
#[ignore]
fn visual_regression() {
    XDialogBuilder::new().run(run_all_captures);
    seed_or_compare("message_info");
    seed_or_compare("progress_50");
}
