//! Offscreen tests for the drawn themes, through this platform's drawing backend
//! (`xdialog::__test::RENDERER`: `d2d` on Windows, `cg` on macOS, `soft` elsewhere).
//!
//! - Determinism: every gallery variant (`examples/gallery/{ubuntu,fluent,macos}.rs`, both macOS
//!   styles) rendered twice in fresh dialogs gives byte-identical frames.
//! - Harness behaviour: hover/press/click/keyboard go through the real `Dialog` path; HiDPI size;
//!   progress and `set_text`.
//! - Accessibility: the AccessKit tree of a message and a progress dialog (snapshots).
//! - Goldens: each theme's `golden` variants vs
//!   `tests/visual_references/offscreen/<renderer>/<theme>/<name>.png` (10/255 per channel, at most
//!   0.5% of pixels). A missing golden fails the test, except for `soft` outside CI, where it is
//!   reported and skipped; `XDIALOG_BLESS=1` (re)writes the goldens (and `FONT_ID`).
//!   - `soft`: always compared (bundled Ubuntu fonts; the Fluent look uses them too).
//!   - `d2d` / `cg`: the renders depend on the system fonts, so they are compared only when
//!     `offscreen/<renderer>/FONT_ID` matches this machine ([`font_id`]); otherwise the test prints
//!     a notice (a `::warning::` when `CI` is set) and skips.
//!
//!   Renders that were not compared (skipped by the font gate) or that differ are written to
//!   `target/offscreen-actual/<renderer>/<theme>/` for inspection (a CI artifact).

#[path = "../examples/gallery/model.rs"]
mod model;
#[path = "../examples/gallery/ubuntu.rs"]
mod ubuntu;
#[path = "../examples/gallery/fluent.rs"]
mod fluent;
#[path = "../examples/gallery/macos.rs"]
mod macos;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub use model::*;
pub use xdialog::__test::{Event, Key, OffscreenDialog, Point, TestAppearance, TestKind, TestMacStyle, TestProgress, RENDERER};
pub use xdialog::{XDialogBackend, XDialogIcon, XDialogOptions, XDialogResult};

const THRESHOLD: u8 = 10;
const MAX_DIFF_FRACTION: f64 = 0.005;

fn assert_deterministic(backend: XDialogBackend, variants: &[Variant]) {
    for v in variants {
        let a = run_variant(backend, v).unwrap_or_else(|e| panic!("{backend:?}/{}: {e}", v.name));
        let b = run_variant(backend, v).unwrap();
        assert_eq!(a.len(), v.captures.len(), "{}: every capture rendered", v.name);
        for (fa, fb) in a.iter().zip(&b) {
            assert_eq!((fa.w, fa.h), (fb.w, fb.h), "{}{}: size", v.name, fa.suffix);
            assert!(fa.rgba == fb.rgba, "{backend:?}/{}{}: two renders of the same script differ", v.name, fa.suffix);
        }
    }
}

#[test]
fn ubuntu_is_deterministic() {
    assert_deterministic(XDialogBackend::Ubuntu, &ubuntu::variants());
}

#[test]
fn fluent_is_deterministic() {
    assert_deterministic(XDialogBackend::Fluent, &fluent::variants());
}

#[test]
fn macos_is_deterministic() {
    assert_deterministic(XDialogBackend::MacOS, &macos::variants());
}

#[test]
fn macos_tahoe_is_deterministic() {
    assert_deterministic(XDialogBackend::MacOS, &macos::tahoe_variants());
}

/// The Tahoe layout against the CFUserNotification captures (macOS 26.6, points): buttons 28
/// tall and 16 from the sides and bottom, side by side 110 wide 8 apart (default on the right),
/// stacked 228 wide every 34 (default on top); the icon at (20, 20), or centred with a heading
/// and no body. With the system font (`cg`) the window heights match the captures too.
#[test]
fn macos_tahoe_layout() {
    let open = |heading: &str, body: &str, icon: XDialogIcon, buttons: &[&str]| {
        OffscreenDialog::with_mac_style(XDialogBackend::MacOS,
                                        TestMacStyle::Tahoe,
                                        look(true),
                                        1.0,
                                        TestKind::Message,
                                        opts("x", heading, body, icon, buttons))
    };
    let close = |a: f64, b: f64| (a - b).abs() < 0.01;
    // (heading, body, icon, buttons, captured window height)
    let cases: [(&str, &str, XDialogIcon, &[&str], u32); 6] =
        [("Are you sure you want to quit Karabiner-Elements?",
          "The changed key will be restored after Karabiner-Elements is quit.",
          XDialogIcon::Warning,
          &["Cancel", "Quit"],
          234),
         ("Update complete", "Your application has been updated.", XDialogIcon::Warning, &["OK"], 202),
         ("Install update?", "A new version is available.", XDialogIcon::Warning, &["Remind Me Later", "Install and Relaunch"], 236),
         ("Do you want to save the changes you made?",
          "Your changes will be lost if you don't save them.",
          XDialogIcon::Information,
          &["Cancel", "Don't Save", "Save"],
          302),
         ("Update failed",
          "Velopack is about to apply an update to this application. The update package has been downloaded and verified, and the \
           application will restart automatically once the installation has finished. Any unsaved work in open windows may be lost if \
           you continue.",
          XDialogIcon::Information,
          &["Cancel", "OK"],
          330),
         ("Hello from maccf-direct!", "", XDialogIcon::Error, &["OK"], 176)];
    for (heading, body, icon, buttons, height) in cases {
        let d = open(heading, body, icon, buttons);
        let (w, h) = d.size_px();
        assert_eq!(w, 260, "{heading}");
        if RENDERER == "cg" {
            assert!(h.abs_diff(height) <= 1, "{heading}: height {h}, captured {height}");
        }
        let r = d.button_rects();
        let bottom = r.iter().map(|b| b[1] + b[3]).fold(0.0, f64::max);
        assert!(close(bottom, h as f64 - 16.0), "{heading}: {r:?} in {h}");
        assert!(r.iter().all(|b| close(b[3], 28.0)), "{heading}: {r:?}");
        match buttons.len() {
            1 => assert!(close(r[0][0], 16.0) && close(r[0][2], 228.0), "{heading}: {r:?}"),
            2 if buttons[0].len() < 10 => {
                assert!(close(r[0][0], 16.0) && close(r[1][0], 134.0), "{heading}: {r:?}");
                assert!(close(r[0][2], 110.0) && close(r[1][2], 110.0) && close(r[0][1], r[1][1]), "{heading}: {r:?}");
            }
            n => {
                // Stacked, the default (last API index) on top, 34 apart.
                for i in 0..n {
                    assert!(close(r[i][0], 16.0) && close(r[i][2], 228.0), "{heading}: {r:?}");
                }
                for i in 1..n {
                    assert!(close(r[i - 1][1] - r[i][1], 34.0), "{heading}: {r:?}");
                }
            }
        }
    }
}

fn two_buttons() -> XDialogOptions {
    opts("t", "Heading", "Body text of the dialog.", XDialogIcon::Warning, &["No", "Yes"])
}

fn px(img: &[u8], w: u32, x: f64, y: f64) -> [u8; 3] {
    let i = ((y as usize) * w as usize + x as usize) * 4;
    [img[i], img[i + 1], img[i + 2]]
}

fn harness_behaviour(backend: XDialogBackend) {
    let mut d = OffscreenDialog::new(backend, look(false), 1.0, TestKind::Message, two_buttons());
    let (w, h) = d.size_px();
    assert!(w >= 200 && h >= 80, "measured size {w}x{h}");
    let rects = d.button_rects();
    assert_eq!(rects.len(), 2);
    assert!(rects.iter().all(|r| r[2] > 10.0 && r[3] > 10.0), "button rects from the measure pass: {rects:?}");

    let (iw, ih, idle) = d.render_at(1.0);
    assert_eq!((iw, ih, idle.len()), (w, h, (w * h * 4) as usize));
    let c = d.button_centre(0).unwrap();
    let probe = (d.button_rects()[0][0] + 6.0, c.y);

    // Hover shows up in the first frame after the event and settles later; a second render at the
    // same time gives the same pixels.
    d.event(Event::PointerMoved(c));
    d.render_at(2.0);
    let (_, _, settled) = d.render_at(3.0);
    assert_eq!(d.render_at(3.0).2, settled);
    assert_ne!(px(&idle, w, probe.0, probe.1), px(&settled, w, probe.0, probe.1), "hover changes the button");

    // Press + release inside -> ButtonPressed; the dialog stops presenting (last image kept).
    d.event(button(c, true));
    d.render_at(3.1);
    assert_eq!(d.result(), None, "press alone does not click");
    d.event(button(c, false));
    let (lw, lh, last) = d.render_at(3.2);
    assert_eq!(d.result(), Some(XDialogResult::ButtonPressed(0)));
    assert_eq!((lw, lh, last.len()), (w, h, (w * h * 4) as usize));

    // Release outside does not click.
    let mut d = OffscreenDialog::new(backend, look(true), 1.0, TestKind::Message, two_buttons());
    d.render_at(0.5);
    let c = d.button_centre(1).unwrap();
    d.event(Event::PointerMoved(c));
    d.event(button(c, true));
    d.render_at(1.0);
    let off = Point::new(2.0, 2.0);
    d.event(Event::PointerMoved(off));
    d.event(button(off, false));
    d.render_at(1.1);
    assert_eq!(d.result(), None);

    // Keyboard: Escape closes with WindowClosed.
    d.event(key(Key::Escape, true));
    d.render_at(1.2);
    assert_eq!(d.result(), Some(XDialogResult::WindowClosed));

    // HiDPI: physical size scales with ppp.
    let d1 = OffscreenDialog::new(backend, look(false), 1.0, TestKind::Message, two_buttons());
    let d2 = OffscreenDialog::new(backend, look(false), 2.0, TestKind::Message, two_buttons());
    let ((w1, h1), (w2, h2)) = (d1.size_px(), d2.size_px());
    assert!(w2.abs_diff(2 * w1) <= 2 && h2.abs_diff(2 * h1) <= 2, "{w1}x{h1} @1 vs {w2}x{h2} @2");
}

#[test]
fn ubuntu_harness_behaviour() {
    harness_behaviour(XDialogBackend::Ubuntu);
}

#[test]
fn fluent_harness_behaviour() {
    harness_behaviour(XDialogBackend::Fluent);
}

#[test]
fn macos_harness_behaviour() {
    harness_behaviour(XDialogBackend::MacOS);
}

#[test]
fn progress_and_text_changes() {
    let mut d = OffscreenDialog::new(XDialogBackend::Ubuntu, look(false), 1.0, TestKind::Progress, opts("p", "Working", "Short.", XDialogIcon::Information, &[]));
    let (_, _, a) = d.render_at(1.0);
    d.set_progress(TestProgress::Value(1.0));
    d.render_at(1.0);
    let (_, _, b) = d.render_at(2.0);
    assert_ne!(a, b, "the bar grew");
    d.set_progress(TestProgress::Indeterminate);
    let (_, _, c1) = d.render_at(3.0);
    let (_, _, c2) = d.render_at(3.5);
    assert_ne!(c1, c2, "indeterminate moves with time");
    let (_, h0) = d.size_px();
    d.set_text(&"A much longer body text that wraps over several lines of the dialog. ".repeat(4));
    d.render_at(4.0);
    let (w1, h1, _) = d.render_at(4.1);
    assert!(h1 > h0, "set_text relayout grows the dialog ({h0} -> {h1})");
    assert_eq!((w1, h1), d.size_px());
}

// ---------------------------------------------------------------------------------------------
// Accessibility
// ---------------------------------------------------------------------------------------------

/// The AccessKit tree of a message dialog: roles, names, focus and physical bounds.
#[test]
fn a11y_tree_of_a_message() {
    for backend in [XDialogBackend::Ubuntu, XDialogBackend::Fluent, XDialogBackend::MacOS] {
        let mut d = OffscreenDialog::new(backend, look(false), 1.0, TestKind::Message, two_buttons());
        d.render_at(1.0);
        let dump = d.a11y_dump();
        println!("{backend:?}:\n{dump}");
        let lines: Vec<&str> = dump.lines().collect();
        assert!(lines[0].starts_with("AlertDialog label=\"t\" description=\"Heading\\nBody text of the dialog.\" [0,0 "), "{dump}");
        assert!(lines[1].starts_with("  Label value=\"Heading\" ["), "{dump}");
        assert!(lines[2].starts_with("  Label value=\"Warning\" ["), "{dump}");
        assert!(lines[3].starts_with("  Label value=\"Body text of the dialog.\" ["), "{dump}");
        // Buttons in Tab order (Fluent puts the default button first); the default one focused.
        let button = |dump: &str, label: &str| {
            let prefix = format!("  Button label=\"{label}\" +Click +Focus [");
            dump.lines().find(|l| l.starts_with(&prefix)).map(str::to_owned)
        };
        let (no, yes) = (button(&dump, "No").expect(&dump), button(&dump, "Yes").expect(&dump));
        assert!(!no.ends_with("(focused)") && yes.ends_with("(focused)"), "{dump}");
        assert_eq!(lines.len(), 6, "{dump}");

        // Focus follows Tab.
        d.event(key(Key::Tab, true));
        d.render_at(1.1);
        let dump2 = d.a11y_dump();
        assert!(button(&dump2, "No").unwrap().ends_with("(focused)"), "{dump2}");

        // Bounds are physical px: twice the logical ones at 2x.
        let bounds = |dump: &str, line: usize| -> [f64; 4] {
            let l = dump.lines().nth(line).unwrap();
            let b = &l[l.find('[').unwrap() + 1..l.find(']').unwrap()];
            let (xy, wh) = b.split_once(' ').unwrap();
            let (x, y) = xy.split_once(',').unwrap();
            let (w, h) = wh.split_once('x').unwrap();
            [x, y, w, h].map(|v| v.parse().unwrap())
        };
        let d2 = OffscreenDialog::new(backend, look(false), 2.0, TestKind::Message, two_buttons());
        let (b1, b2) = (bounds(&dump, 2), bounds(&d2.a11y_dump(), 2));
        assert!(b1.iter().zip(b2).all(|(a, b)| (b - 2.0 * a).abs() <= 1.0), "{backend:?}: icon {b1:?} @1 vs {b2:?} @2");
    }
}

/// No icon node for `None`; a blank title names the dialog after the heading.
#[test]
fn a11y_tree_without_icon_or_title() {
    let options = opts(" ", "Heading", "Body.", XDialogIcon::None, &["OK"]);
    let d = OffscreenDialog::new(XDialogBackend::Ubuntu, look(false), 1.0, TestKind::Message, options);
    let dump = d.a11y_dump();
    let lines: Vec<&str> = dump.lines().collect();
    assert!(lines[0].starts_with("AlertDialog label=\"Heading\" description=\"Heading\\nBody.\""), "{dump}");
    assert_eq!(lines.len(), 4, "{dump}");
    assert!(lines[3].starts_with("  Button label=\"OK\" +Click +Focus ["), "{dump}");
}

/// The AccessKit tree of a progress dialog: the value, then none while indeterminate.
#[test]
fn a11y_tree_of_a_progress() {
    let options = opts("Installing", "Installing update", "Downloading...", XDialogIcon::None, &["Cancel"]);
    for backend in [XDialogBackend::Fluent, XDialogBackend::Ubuntu, XDialogBackend::MacOS] {
        let mut d = OffscreenDialog::new(backend, look(true), 1.0, TestKind::Progress, options.clone());
        d.set_progress(TestProgress::Value(0.42));
        d.render_at(1.0);
        let dump = d.a11y_dump();
        println!("{backend:?}:\n{dump}");
        let lines: Vec<&str> = dump.lines().collect();
        assert!(lines[0].starts_with("Dialog label=\"Installing\" description=\"Installing update\\nDownloading...\""), "{dump}");
        assert!(lines[1].starts_with("  Label value=\"Installing update\""), "{dump}");
        assert!(lines[2].starts_with("  Label value=\"Downloading...\""), "{dump}");
        assert!(lines[3].starts_with("  ProgressIndicator value=\"42%\" numeric=42 ["), "{dump}");
        assert!(lines[4].starts_with("  Button label=\"Cancel\" +Click +Focus ["), "{dump}");
        assert_eq!(lines.len(), 5, "{dump}");
        d.set_progress(TestProgress::Indeterminate);
        d.render_at(1.5);
        let dump = d.a11y_dump();
        assert!(dump.lines().nth(3).unwrap().starts_with("  ProgressIndicator ["), "{dump}");
    }
}

// ---------------------------------------------------------------------------------------------
// Goldens
// ---------------------------------------------------------------------------------------------

fn renderer_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/visual_references/offscreen").join(RENDERER)
}

fn golden_dir(theme: &str) -> PathBuf {
    renderer_dir().join(theme)
}

/// Where renders that were not compared, or that differ, are written.
fn actual_dir(theme: &str) -> PathBuf {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    tmp.parent().unwrap_or(tmp).join("offscreen-actual").join(RENDERER).join(theme)
}

fn blessing() -> bool {
    std::env::var("XDIALOG_BLESS").is_ok_and(|v| v == "1")
}

/// What the renders of this renderer depend on besides xdialog: `None` for `soft` (bundled fonts),
/// otherwise the system UI fonts' byte lengths and the OS version.
fn font_id() -> Option<String> {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct OsVersionInfoW {
            size: u32,
            major: u32,
            minor: u32,
            build: u32,
            platform: u32,
            csd: [u16; 128],
        }
        #[link(name = "ntdll")]
        extern "system" {
            fn RtlGetVersion(info: *mut OsVersionInfoW) -> i32;
        }
        let size = std::mem::size_of::<OsVersionInfoW>() as u32;
        let mut v = OsVersionInfoW { size, major: 0, minor: 0, build: 0, platform: 0, csd: [0; 128] };
        // SAFETY: `v` is a valid OSVERSIONINFOW with its size set; RtlGetVersion only writes it.
        unsafe { RtlGetVersion(&mut v) };
        let fonts = Path::new(&std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into())).join("Fonts");
        let len = |f: &str| std::fs::metadata(fonts.join(f)).map_or(0, |m| m.len());
        Some(format!("segoeui.ttf={} SegUIVar.ttf={} build={}", len("segoeui.ttf"), len("SegUIVar.ttf"), v.build))
    }
    #[cfg(target_os = "macos")]
    {
        let len = std::fs::metadata("/System/Library/Fonts/SFNS.ttf").map_or(0, |m| m.len());
        let version = std::process::Command::new("sw_vers").arg("-productVersion").output();
        let version = version.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        let major = version.trim().split('.').next().unwrap_or("").to_owned();
        Some(format!("SFNS.ttf={len} macos={major}"))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

/// Whether the goldens of this renderer apply to this machine (always for `soft`). Blessing
/// writes `FONT_ID`. Decided once per process: the golden tests run in parallel.
fn goldens_apply() -> bool {
    static APPLY: OnceLock<bool> = OnceLock::new();
    *APPLY.get_or_init(|| {
              let Some(id) = font_id() else { return true };
              let record = renderer_dir().join("FONT_ID");
              if blessing() {
                  std::fs::create_dir_all(renderer_dir()).unwrap();
                  std::fs::write(&record, format!("{id}\n")).unwrap();
                  return true;
              }
              let recorded = std::fs::read_to_string(&record).map(|s| s.trim().to_owned()).unwrap_or_default();
              if recorded != id {
                  let notice = format!("{RENDERER}: goldens skipped: they were made with `{recorded}`, this machine has `{id}`");
                  // On CI, a warning makes a runner image change visible in the run summary.
                  let prefix = if std::env::var_os("CI").is_some() { "::warning::" } else { "" };
                  println!("{prefix}{notice}");
                  return false;
              }
              true
          })
}

fn save(dir: &Path, name: &str, f: &Frame) {
    std::fs::create_dir_all(dir).unwrap();
    image::save_buffer(dir.join(name), &f.rgba, f.w, f.h, image::ColorType::Rgba8).unwrap();
}

/// Compare the golden variants of `backend` against the goldens in `theme` (the backend's name,
/// or `macos_tahoe`); returns the names compared (or blessed).
fn check_goldens(backend: XDialogBackend, theme: &str, variants: &[Variant]) -> Vec<String> {
    let (dir, actual) = (golden_dir(theme), actual_dir(theme));
    let apply = goldens_apply();
    let (mut done, mut skipped, mut failures) = (Vec::new(), Vec::new(), Vec::new());
    for v in variants.iter().filter(|v| v.golden) {
        for f in run_variant(backend, v).unwrap_or_else(|e| panic!("{e}")) {
            let name = format!("{}{}.png", v.name, f.suffix);
            if blessing() {
                save(&dir, &name, &f);
                done.push(name);
                continue;
            }
            if !apply {
                save(&actual, &name, &f);
                continue;
            }
            let Ok(img) = image::open(dir.join(&name)) else {
                save(&actual, &name, &f);
                skipped.push(name);
                continue;
            };
            let img = img.to_rgba8();
            if (img.width(), img.height()) != (f.w, f.h) {
                save(&actual, &name, &f);
                failures.push(format!("{name}: size {}x{} vs golden {}x{}", f.w, f.h, img.width(), img.height()));
                continue;
            }
            let over = f.rgba.chunks(4).zip(img.as_raw().chunks(4)).filter(|(a, b)| (0..3).any(|i| a[i].abs_diff(b[i]) > THRESHOLD)).count();
            let frac = over as f64 / (f.w * f.h) as f64;
            if frac > MAX_DIFF_FRACTION {
                save(&actual, &name, &f);
                failures.push(format!("{name}: {:.2}% of pixels differ", frac * 100.0));
            }
            done.push(name);
        }
    }
    if !skipped.is_empty() {
        println!("{RENDERER}/{theme}: no golden yet (skipped; XDIALOG_BLESS=1 writes them): {}", skipped.join(", "));
    }
    assert!(failures.is_empty(), "{RENDERER}/{theme} goldens differ (renders in {}):\n{}", actual.display(), failures.join("\n"));
    // A golden that was never added must not pass unnoticed. d2d/cg goldens are compared only where
    // FONT_ID matches (never on CI), so a local run is their only check.
    let strict = RENDERER != "soft" || std::env::var_os("CI").is_some();
    assert!(skipped.is_empty() || !strict, "{RENDERER}/{theme}: goldens missing: {}", skipped.join(", "));
    done
}

#[test]
fn ubuntu_goldens() {
    let done = check_goldens(XDialogBackend::Ubuntu, "ubuntu", &ubuntu::variants());
    println!("{RENDERER}/ubuntu: {} goldens {}", done.len(), if blessing() { "written" } else { "compared" });
}

#[test]
fn fluent_goldens() {
    let done = check_goldens(XDialogBackend::Fluent, "fluent", &fluent::variants());
    println!("{RENDERER}/fluent: {} goldens {}", done.len(), if blessing() { "written" } else { "compared" });
}

#[test]
fn macos_goldens() {
    let done = check_goldens(XDialogBackend::MacOS, "macos", &macos::variants());
    println!("{RENDERER}/macos: {} goldens {}", done.len(), if blessing() { "written" } else { "compared" });
}

#[test]
fn macos_tahoe_goldens() {
    let done = check_goldens(XDialogBackend::MacOS, "macos_tahoe", &macos::tahoe_variants());
    println!("{RENDERER}/macos_tahoe: {} goldens {}", done.len(), if blessing() { "written" } else { "compared" });
}
