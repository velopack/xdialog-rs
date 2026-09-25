//! On-screen captures of the native backends (Win32 TaskDialog, AppKit) vs
//! `tests/visual_references/<windows|macos>/`; the egui looks have offscreen goldens
//! (`tests/egui_offscreen.rs`). Opt-in, it takes focus: `XDIALOG_VISUAL_TEST=1` compares,
//! `XDIALOG_VISUAL_SEED=1 cargo test --test visual_regression` (re)writes the references. A failing
//! capture and its diff are written to `target/tmp/visual_output/`. `harness = false`: one backend
//! per process, macOS wants the UI on the main thread.

use std::path::Path;
use std::thread;
use std::time::Duration;

use image::{Rgba, RgbaImage};
use xdialog::*;

const TITLE: &str = "XDialog Visual Test";
/// Time a dialog gets to render before the first capture attempt.
const RENDER_WAIT: Duration = Duration::from_millis(1500);
/// Per-channel difference (0-255) up to which pixels count as equal (anti-aliasing).
const PIXEL_THRESHOLD: u8 = 10;
/// Fraction of differing pixels from which a capture fails.
const MAX_DIFF_FRACTION: f64 = 0.05;
/// Border ignored by the comparison: the capture includes the window shadow / what's behind it.
const EDGE_MARGIN: u32 = 16;

/// `try_capture(title)`: one attempt at capturing the window with that exact title.
#[cfg(windows)]
mod capture {
    use super::*;
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// The window's screen area from the screen DC (includes DWM compositing).
    pub fn try_capture(title: &str) -> Option<RgbaImage> {
        unsafe {
            let hwnd = FindWindowW(None, &windows::core::HSTRING::from(title)).ok()?;
            // Focus the desktop so the title bar is always captured inactive (as on CI).
            let _ = SetForegroundWindow(GetDesktopWindow());
            thread::sleep(Duration::from_millis(100));

            let mut rect = RECT::default();
            GetWindowRect(hwnd, &mut rect).ok()?;
            let (width, height) = ((rect.right - rect.left) as u32, (rect.bottom - rect.top) as u32);
            if width == 0 || height == 0 {
                return None;
            }
            let hdc_screen = GetDC(None);
            let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
            let hbitmap = CreateCompatibleBitmap(hdc_screen, width as i32, height as i32);
            let old_bitmap = SelectObject(hdc_mem, hbitmap.into());
            let _ = BitBlt(hdc_mem, 0, 0, width as i32, height as i32, Some(hdc_screen), rect.left, rect.top, SRCCOPY);
            let mut bmi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32,
                                                                     biWidth: width as i32,
                                                                     biHeight: -(height as i32), // top-down
                                                                     biPlanes: 1,
                                                                     biBitCount: 32,
                                                                     ..Default::default() },
                                       ..Default::default() };
            let mut pixels = vec![0u8; (width * height * 4) as usize];
            GetDIBits(hdc_mem, hbitmap, 0, height, Some(pixels.as_mut_ptr().cast()), &mut bmi, DIB_RGB_COLORS);
            SelectObject(hdc_mem, old_bitmap);
            let _ = DeleteObject(hbitmap.into());
            let _ = DeleteDC(hdc_mem);
            ReleaseDC(None, hdc_screen);

            // BGRA -> RGBA
            for px in pixels.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
            RgbaImage::from_raw(width, height, pixels)
        }
    }
}

/// AppKit: the window by title, through xcap.
#[cfg(target_os = "macos")]
mod capture {
    use super::*;

    /// Resign our app's active status (by activating Finder) so the dialog is captured inactive,
    /// like the Windows path: whether it is "key" on a CI session is nondeterministic (default
    /// button highlight, title bar shade).
    pub fn try_capture(title: &str) -> Option<RgbaImage> {
        let _ = std::process::Command::new("osascript").args(["-e", "tell application \"Finder\" to activate"]).status();
        thread::sleep(Duration::from_millis(250));
        let windows = xcap::Window::all().map_err(|e| eprintln!("Failed to list windows: {e}")).ok()?;
        let window = windows.iter().find(|w| w.title().ok().as_deref() == Some(title))?;
        window.capture_image().map_err(|e| eprintln!("Failed to capture window: {e}")).ok()
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod capture {
    pub fn try_capture(_title: &str) -> Option<super::RgbaImage> {
        None
    }
}

/// The window titled [`TITLE`], retried for up to 10 s.
fn capture_window() -> RgbaImage {
    for _ in 0..20 {
        if let Some(image) = capture::try_capture(TITLE) {
            return image;
        }
        thread::sleep(Duration::from_millis(500));
    }
    panic!("window '{TITLE}' not found after 10 s");
}

/// The dialogs to compare (runs under `XDialogBuilder`).
fn run_all_captures() -> Vec<(&'static str, RgbaImage)> {
    // A blocking message dialog: captured from another thread, closed by its timeout.
    let capturing = thread::spawn(|| {
        thread::sleep(RENDER_WAIT);
        capture_window()
    });
    let options = XDialogOptions { title: TITLE.to_string(),
                                   main_instruction: "Information".to_string(),
                                   message: "This is a test message for visual regression testing.".to_string(),
                                   icon: XDialogIcon::Information,
                                   buttons: vec!["OK".to_string()] };
    let _ = show_message(options, Some(Duration::from_secs(15)));
    let message = capturing.join().unwrap();

    let progress = show_progress(TITLE, "Working...", "Downloading update...", XDialogIcon::Information).unwrap();
    progress.set_value(0.0).unwrap();
    thread::sleep(RENDER_WAIT);
    let progress_0 = capture_window();
    progress.close().unwrap();
    vec![("message_info", message), ("progress_0", progress_0)]
}

/// Fraction of the pixels inside [`EDGE_MARGIN`] that differ, and a diff image (red: differs).
fn diff(actual: &RgbaImage, expected: &RgbaImage) -> (f64, RgbaImage) {
    let (w, h) = actual.dimensions();
    let (xs, ys) = (EDGE_MARGIN..w.saturating_sub(EDGE_MARGIN), EDGE_MARGIN..h.saturating_sub(EDGE_MARGIN));
    let (mut compared, mut differing) = (0usize, 0usize);
    let image = RgbaImage::from_fn(w, h, |x, y| {
        let (a, e) = (actual.get_pixel(x, y), expected.get_pixel(x, y));
        if !xs.contains(&x) || !ys.contains(&y) {
            return Rgba([80, 80, 80, 255]);
        }
        compared += 1;
        if (0..3).any(|i| a[i].abs_diff(e[i]) > PIXEL_THRESHOLD) {
            differing += 1;
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([a[0] / 3, a[1] / 3, a[2] / 3, 255])
        }
    });
    (differing as f64 / compared.max(1) as f64, image)
}

fn main() {
    let seed = std::env::var_os("XDIALOG_VISUAL_SEED").is_some();
    if (!seed && std::env::var_os("XDIALOG_VISUAL_TEST").is_none()) || !cfg!(any(windows, target_os = "macos")) {
        eprintln!("Skipping visual regression (Windows and macOS; set XDIALOG_VISUAL_TEST=1 or XDIALOG_VISUAL_SEED=1)");
        return;
    }
    // The references are the native backends (`Auto` picks Fluent on Windows 10+).
    let backend = if cfg!(windows) { XDialogBackend::Win32 } else { XDialogBackend::Auto };
    let captures = XDialogBuilder::new().with_backend(backend).run_loop(run_all_captures);

    let refs = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/visual_references").join(if cfg!(windows) { "windows" } else { "macos" });
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("visual_output");
    let mut failures = Vec::new();
    for (name, actual) in captures {
        let reference = refs.join(format!("{name}.png"));
        if seed {
            std::fs::create_dir_all(&refs).unwrap();
            actual.save(&reference).unwrap();
            eprintln!("Seeded {}", reference.display());
            continue;
        }
        let expected = image::open(&reference).unwrap_or_else(|e| panic!("{}: {e} (XDIALOG_VISUAL_SEED=1 seeds it)", reference.display())).to_rgba8();
        let (fraction, diff_image) = if actual.dimensions() == expected.dimensions() { diff(&actual, &expected) } else { (1.0, actual.clone()) };
        if fraction >= MAX_DIFF_FRACTION {
            std::fs::create_dir_all(&out).unwrap();
            actual.save(out.join(format!("{name}.png"))).unwrap();
            diff_image.save(out.join(format!("{name}.diff.png"))).unwrap();
            failures.push(format!("{name}: {:.2}% of pixels differ ({:?} vs reference {:?})", fraction * 100.0, actual.dimensions(), expected.dimensions()));
        }
    }
    assert!(failures.is_empty(), "visual regression (captures in {}):\n{}", out.display(), failures.join("\n"));
}
