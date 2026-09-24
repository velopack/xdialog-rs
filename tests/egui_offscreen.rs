//! Offscreen tests for the egui themes.
//!
//! - Determinism: every gallery variant (`examples/egui_gallery/{linux,fluent}.rs`) rendered twice
//!   in fresh dialogs gives byte-identical frames.
//! - Harness behaviour: hover/press/click/keyboard go through the real `Dialog` path; HiDPI size.
//! - Linux design tokens: background, focus ring after the pointer leaves / in an inactive window,
//!   the accent overlay, sampled away from corners with a loose tolerance.
//! - Goldens: each theme's `golden` variants vs `tests/visual_references/egui/<theme>/<name>.png`
//!   (10/255 per channel, at most 0.5% of pixels). A missing golden is reported and skipped;
//!   `XDIALOG_VISUAL_SEED=1` (re)writes them. Linux goldens run everywhere (bundled font). Fluent
//!   goldens are local-only: they run only when `C:\Windows\Fonts\SegUIVar.ttf` exists and its
//!   byte length matches `tests/visual_references/egui/fluent/FONT_ID` (`len=<n>`, written by
//!   seeding).

#[path = "../examples/egui_gallery/model.rs"]
mod model;
#[path = "../examples/egui_gallery/linux.rs"]
mod linux;
#[cfg(feature = "fluent-egui")]
#[path = "../examples/egui_gallery/fluent.rs"]
mod fluent;

use std::path::{Path, PathBuf};

pub use model::*;
pub use xdialog::__test::{HostEvent, Key, MouseButton, OffscreenDialog, TestAppearance, TestKind, TestProgress};
pub use xdialog::{XDialogIcon, XDialogOptions, XDialogResult};

const THRESHOLD: u8 = 10;
const MAX_DIFF_FRACTION: f64 = 0.005;

fn assert_deterministic(theme: &str, variants: &[Variant]) {
    for v in variants {
        let a = run_variant(theme, v).unwrap_or_else(|e| panic!("{theme}/{}: {e}", v.name));
        let b = run_variant(theme, v).unwrap();
        assert_eq!(a.len(), v.captures.len(), "{}: every capture rendered", v.name);
        for (fa, fb) in a.iter().zip(&b) {
            assert_eq!((fa.w, fa.h), (fb.w, fb.h), "{}{}: size", v.name, fa.suffix);
            assert!(fa.rgba == fb.rgba, "{theme}/{}{}: two renders of the same script differ", v.name, fa.suffix);
        }
    }
}

#[test]
fn linux_is_deterministic() {
    assert_deterministic("linux", &linux::variants());
}

#[cfg(feature = "fluent-egui")]
#[test]
fn fluent_is_deterministic() {
    assert_deterministic("fluent", &fluent::variants());
}

fn two_buttons() -> XDialogOptions {
    opts("t", "Heading", "Body text of the dialog.", XDialogIcon::Warning, &["No", "Yes"])
}

fn px(img: &[u8], w: u32, x: f64, y: f64) -> [u8; 3] {
    let i = ((y as usize) * w as usize + x as usize) * 4;
    [img[i], img[i + 1], img[i + 2]]
}

fn harness_behaviour(theme: &str) {
    let mut d = OffscreenDialog::new(theme, look(false), 1.0, TestKind::Message, two_buttons()).unwrap();
    let (w, h) = d.size_px();
    assert!(w >= 200 && h >= 80, "measured size {w}x{h}");
    let rects = d.button_rects();
    assert_eq!(rects.len(), 2);
    assert!(rects.iter().all(|r| r[2] > 10.0 && r[3] > 10.0), "button rects from the measure pass: {rects:?}");

    let idle = d.render_at(1.0);
    assert_eq!(idle.len(), (w * h * 4) as usize);
    assert_eq!(d.last_size(), (w, h));
    let (cx, cy) = d.button_centre_px(0).unwrap();
    let probe = (d.button_rects()[0][0] as f64 + 6.0, cy);

    // Hover shows up in the first frame after the event and settles later; a second render at the
    // same time gives the same pixels.
    d.event(HostEvent::CursorMoved { x: cx, y: cy });
    d.render_at(2.0);
    let settled = d.render_at(3.0);
    assert_eq!(d.render_at(3.0), settled);
    assert_ne!(px(&idle, w, probe.0, probe.1), px(&settled, w, probe.0, probe.1), "hover changes the button");

    // Press + release inside -> ButtonPressed; the dialog stops presenting (last image kept).
    d.event(HostEvent::MouseButton { button: MouseButton::Primary, pressed: true });
    d.render_at(3.1);
    assert_eq!(d.result(), None, "press alone does not click");
    d.event(HostEvent::MouseButton { button: MouseButton::Primary, pressed: false });
    let last = d.render_at(3.2);
    assert_eq!(d.result(), Some(XDialogResult::ButtonPressed(0)));
    assert_eq!(last.len(), (w * h * 4) as usize);

    // Release outside does not click.
    let mut d = OffscreenDialog::new(theme, look(true), 1.0, TestKind::Message, two_buttons()).unwrap();
    d.render_at(0.5);
    let (cx, cy) = d.button_centre_px(1).unwrap();
    d.event(HostEvent::CursorMoved { x: cx, y: cy });
    d.event(HostEvent::MouseButton { button: MouseButton::Primary, pressed: true });
    d.render_at(1.0);
    d.event(HostEvent::CursorMoved { x: 2.0, y: 2.0 });
    d.event(HostEvent::MouseButton { button: MouseButton::Primary, pressed: false });
    d.render_at(1.1);
    assert_eq!(d.result(), None);

    // Keyboard: Escape closes with WindowClosed.
    d.event(HostEvent::Key { key: Key::Escape, pressed: true, repeat: false });
    d.render_at(1.2);
    assert_eq!(d.result(), Some(XDialogResult::WindowClosed));

    // HiDPI: physical size scales with ppp.
    let d1 = OffscreenDialog::new(theme, look(false), 1.0, TestKind::Message, two_buttons()).unwrap();
    let d2 = OffscreenDialog::new(theme, look(false), 2.0, TestKind::Message, two_buttons()).unwrap();
    let ((w1, h1), (w2, h2)) = (d1.size_px(), d2.size_px());
    assert!(w2.abs_diff(2 * w1) <= 2 && h2.abs_diff(2 * h1) <= 2, "{w1}x{h1} @1 vs {w2}x{h2} @2");

    // Unknown theme / bad ppp are errors.
    assert!(OffscreenDialog::new("nope", look(false), 1.0, TestKind::Message, two_buttons()).is_err());
    assert!(OffscreenDialog::new(theme, look(false), 0.0, TestKind::Message, two_buttons()).is_err());
}

#[test]
fn linux_harness_behaviour() {
    harness_behaviour("linux");
}

#[cfg(feature = "fluent-egui")]
#[test]
fn fluent_harness_behaviour() {
    harness_behaviour("fluent");
}

#[test]
fn progress_and_text_changes() {
    let mut d = OffscreenDialog::new("linux", look(false), 1.0, TestKind::Progress, opts("p", "Working", "Short.", XDialogIcon::Information, &[])).unwrap();
    let a = d.render_at(1.0);
    d.set_progress(TestProgress::Value(1.0));
    d.render_at(1.0);
    let b = d.render_at(2.0);
    assert_ne!(a, b, "the bar grew");
    d.set_progress(TestProgress::Indeterminate);
    let c1 = d.render_at(3.0);
    let c2 = d.render_at(3.5);
    assert_ne!(c1, c2, "indeterminate moves with time");
    let (_, h0) = d.size_px();
    d.set_text(&"A much longer body text that wraps over several lines of the dialog. ".repeat(4));
    d.render_at(4.0);
    d.render_at(4.1);
    assert!(d.size_px().1 > h0, "set_text relayout grows the dialog ({} -> {})", h0, d.size_px().1);
    assert_eq!(d.last_size(), d.size_px());
}

// ---------------------------------------------------------------------------------------------
// Goldens
// ---------------------------------------------------------------------------------------------

fn golden_dir(theme: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/visual_references/egui").join(theme)
}

fn seeding() -> bool {
    std::env::var("XDIALOG_VISUAL_SEED").is_ok_and(|v| v == "1")
}

/// Compare `theme`'s golden variants; returns (compared, skipped) names.
fn check_goldens(theme: &str, variants: &[Variant]) -> (Vec<String>, Vec<String>) {
    let dir = golden_dir(theme);
    let (mut done, mut skipped, mut failures) = (Vec::new(), Vec::new(), Vec::new());
    for v in variants.iter().filter(|v| v.golden) {
        for f in run_variant(theme, v).unwrap_or_else(|e| panic!("{e}")) {
            let name = format!("{}{}.png", v.name, f.suffix);
            let path = dir.join(&name);
            if seeding() {
                std::fs::create_dir_all(&dir).unwrap();
                image::save_buffer(&path, &f.rgba, f.w, f.h, image::ColorType::Rgba8).unwrap();
                done.push(name);
                continue;
            }
            let Ok(img) = image::open(&path) else {
                skipped.push(name);
                continue;
            };
            let img = img.to_rgba8();
            if (img.width(), img.height()) != (f.w, f.h) {
                failures.push(format!("{name}: size {}x{} vs golden {}x{}", f.w, f.h, img.width(), img.height()));
                continue;
            }
            let over = f.rgba.chunks(4).zip(img.as_raw().chunks(4)).filter(|(a, b)| (0..3).any(|i| a[i].abs_diff(b[i]) > THRESHOLD)).count();
            let frac = over as f64 / (f.w * f.h) as f64;
            if frac > MAX_DIFF_FRACTION {
                failures.push(format!("{name}: {:.2}% of pixels differ", frac * 100.0));
            }
            done.push(name);
        }
    }
    if !skipped.is_empty() {
        println!("{theme}: no golden yet (skipped; XDIALOG_VISUAL_SEED=1 seeds): {}", skipped.join(", "));
    }
    assert!(failures.is_empty(), "{theme} goldens differ:\n{}", failures.join("\n"));
    (done, skipped)
}

#[test]
fn linux_goldens() {
    let (done, _) = check_goldens("linux", &linux::variants());
    println!("linux: {} goldens {}", done.len(), if seeding() { "seeded" } else { "compared" });
}

#[cfg(feature = "fluent-egui")]
#[test]
fn fluent_goldens() {
    let Ok(meta) = std::fs::metadata(r"C:\Windows\Fonts\SegUIVar.ttf") else {
        println!("skipped: Segoe UI Variable missing or a different version");
        return;
    };
    let id = format!("len={}", meta.len());
    let record = golden_dir("fluent").join("FONT_ID");
    if seeding() {
        std::fs::create_dir_all(golden_dir("fluent")).unwrap();
        std::fs::write(&record, format!("{id}\n")).unwrap();
    } else if std::fs::read_to_string(&record).map(|s| s.trim().to_owned()).ok().as_deref() != Some(id.as_str()) {
        println!("skipped: Segoe UI Variable missing or a different version");
        return;
    }
    let (done, _) = check_goldens("fluent", &fluent::variants());
    println!("fluent: {} goldens {}", done.len(), if seeding() { "seeded" } else { "compared" });
}

// ---------------------------------------------------------------------------------------------
// Linux theme design tokens (sampled away from corners: the shapes may change, the colours not)
// ---------------------------------------------------------------------------------------------

const BG: ([u8; 3], [u8; 3]) = ([0xFA, 0xFA, 0xFA], [0x2D, 0x2D, 0x2D]);
const IDLE_FILL: ([u8; 3], [u8; 3]) = ([0xFF, 0xFF, 0xFF], [0x3B, 0x3B, 0x3B]);
const IDLE_BORDER: ([u8; 3], [u8; 3]) = ([0xC7, 0xC7, 0xC7], [0x5A, 0x5A, 0x5A]);
const ACCENT: [u8; 3] = [0x2A, 0x7D, 0xE3];
const ORANGE: [u8; 3] = [0xE9, 0x54, 0x20];
const TOL: u8 = 6;

fn pick(c: ([u8; 3], [u8; 3]), dark: bool) -> [u8; 3] {
    if dark {
        c.1
    } else {
        c.0
    }
}

/// A settled frame plus the button rects (logical) it was laid out with.
struct Shot {
    rgba: Vec<u8>,
    w: u32,
    h: u32,
    ppp: f32,
    rects: Vec<[f32; 4]>,
}

impl Shot {
    /// Renders at `t` and again 0.5 s later, when every transition has finished.
    fn take(d: &mut OffscreenDialog, t: f64) -> Shot {
        d.render_at(t);
        let rgba = d.render_at(t + 0.5);
        let (w, h) = d.last_size();
        Shot { rgba, w, h, ppp: d.ppp(), rects: d.button_rects() }
    }

    /// RGB at a logical point.
    fn at(&self, x: f32, y: f32) -> [u8; 3] {
        px(&self.rgba, self.w, (x * self.ppp).floor() as f64, (y * self.ppp).floor() as f64)
    }

    /// Button `i`'s border at the top-edge midpoint: of the pixels 1 px above, on and 1 px below
    /// the edge, the one closest to `want` (the stroke may sit on, inside or across the edge).
    fn border(&self, i: usize, want: [u8; 3]) -> [u8; 3] {
        let r = self.rects[i];
        let dist = |c: [u8; 3]| (0..3).map(|k| c[k].abs_diff(want[k])).max().unwrap();
        [-1.0, 0.0, 1.0].map(|dy| self.at(r[0] + r[2] / 2.0, r[1] + dy)).into_iter().min_by_key(|&c| dist(c)).unwrap()
    }

    /// Button `i`'s fill: 6 px inside the left edge at mid-height.
    fn fill(&self, i: usize) -> [u8; 3] {
        let r = self.rects[i];
        self.at(r[0] + 6.0, r[1] + r[3] / 2.0)
    }

    /// Pixels within `TOL` of `c`.
    fn count(&self, c: [u8; 3]) -> usize {
        self.rgba.chunks(4).filter(|p| close(&p[..3], c)).count()
    }
}

fn close(a: &[u8], b: [u8; 3]) -> bool {
    (0..3).all(|i| a[i].abs_diff(b[i]) <= TOL)
}

#[track_caller]
fn assert_colour(what: &str, got: [u8; 3], want: [u8; 3]) {
    assert!(close(&got, want), "{what}: got {got:02X?}, want {want:02X?} (±{TOL})");
}

#[track_caller]
fn assert_border(what: &str, s: &Shot, i: usize, want: [u8; 3]) {
    assert_colour(what, s.border(i, want), want);
}

fn states_dialog(appearance: TestAppearance) -> OffscreenDialog {
    OffscreenDialog::new("linux", appearance, 1.0, TestKind::Message, two_buttons()).unwrap()
}

const NO: usize = 0;
const YES: usize = 1;

#[test]
fn linux_focus_ring_returns_after_pointer_leaves() {
    for dark in [false, true] {
        let mut d = states_dialog(look(dark));
        d.render_at(0.5);
        let (x, y) = d.button_centre_px(NO).unwrap();
        d.event(HostEvent::CursorMoved { x, y });
        let hovered = Shot::take(&mut d, 1.0);
        assert_colour("hovered No fill", hovered.fill(NO), ACCENT);
        assert_border("Yes ring hidden while No is hovered", &hovered, YES, pick(IDLE_BORDER, dark));
        d.event(HostEvent::CursorLeft);
        let left = Shot::take(&mut d, 2.0);
        assert_colour("No fill after leave", left.fill(NO), pick(IDLE_FILL, dark));
        assert_border("Yes ring after leave", &left, YES, ACCENT);
    }
}

#[test]
fn linux_inactive_window_keeps_focus_ring() {
    for dark in [false, true] {
        let mut d = states_dialog(look(dark));
        d.render_at(0.2);
        d.event(HostEvent::Focused(false));
        let s = Shot::take(&mut d, 1.0);
        assert_border("Yes ring", &s, YES, ACCENT);
        assert_colour("Yes fill", s.fill(YES), pick(IDLE_FILL, dark));
        assert_border("No border", &s, NO, pick(IDLE_BORDER, dark));
    }
}

#[test]
fn linux_accent_overlay() {
    for dark in [false, true] {
        let accented = TestAppearance { accent: Some(ORANGE), ..look(dark) };
        let mut d = states_dialog(accented.clone());
        let focus = Shot::take(&mut d, 0.5);
        assert_border("focus ring", &focus, YES, ORANGE);
        let (x, y) = d.button_centre_px(NO).unwrap();
        d.event(HostEvent::CursorMoved { x, y });
        let hover = Shot::take(&mut d, 1.5);
        assert_colour("hover fill", hover.fill(NO), ORANGE);

        // Progress: the bar takes the accent, the track is 35% accent over the background.
        let mut p = OffscreenDialog::new("linux", accented, 1.0, TestKind::Progress, opts("p", "Working", "Short.", XDialogIcon::Information, &[])).unwrap();
        p.set_progress(TestProgress::Value(0.5));
        let s = Shot::take(&mut p, 1.0);
        let bg = pick(BG, dark);
        let track = [0, 1, 2].map(|i| (ORANGE[i] as f32 * 0.35 + bg[i] as f32 * 0.65).round() as u8);
        assert!(s.count(ORANGE) > 300, "progress fill in the accent colour ({} px)", s.count(ORANGE));
        assert!(s.count(track) > 300, "track at 35% accent {track:02X?} ({} px)", s.count(track));
        assert_eq!(s.count(ACCENT), 0, "no default-blue pixels left");
    }
}

#[test]
fn linux_background_token() {
    for dark in [false, true] {
        let mut d = states_dialog(look(dark));
        let s = Shot::take(&mut d, 1.0);
        let (lw, lh) = (s.w as f32 / s.ppp, s.h as f32 / s.ppp);
        for (x, y) in [(2.0, 2.0), (lw - 3.0, 2.0), (2.0, lh - 3.0), (lw / 2.0, lh - 3.0)] {
            assert_colour(&format!("background at ({x}, {y})"), s.at(x, y), pick(BG, dark));
        }
    }
}
