//! Offscreen tests for the egui themes.
//!
//! - Determinism: every gallery variant (`examples/egui_gallery/{ubuntu,fluent}.rs`) rendered twice
//!   in fresh dialogs gives byte-identical frames.
//! - Harness behaviour: hover/press/click/keyboard go through the real `Dialog` path; HiDPI size.
//! - Goldens: each theme's `golden` variants vs `tests/visual_references/egui/<theme>/<name>.png`
//!   (10/255 per channel, at most 0.5% of pixels). A missing golden is reported and skipped (a
//!   failure when `CI` is set); `XDIALOG_VISUAL_SEED=1` (re)writes them. Ubuntu goldens run everywhere (bundled font). Fluent
//!   goldens are local-only: they run only when `C:\Windows\Fonts\SegUIVar.ttf` exists and its
//!   byte length matches `tests/visual_references/egui/fluent/FONT_ID` (`len=<n>`, written by
//!   seeding).

#[path = "../examples/egui_gallery/model.rs"]
mod model;
#[path = "../examples/egui_gallery/ubuntu.rs"]
mod ubuntu;
#[path = "../examples/egui_gallery/fluent.rs"]
mod fluent;

use std::path::{Path, PathBuf};

pub use model::*;
pub use xdialog::__test::egui::{Event, Key, Pos2};
pub use xdialog::__test::{OffscreenDialog, TestAppearance, TestKind, TestProgress};
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
    let probe = (d.button_rects()[0][0] as f64 + 6.0, c.y as f64);

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
    let off = Pos2::new(2.0, 2.0);
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
// Goldens
// ---------------------------------------------------------------------------------------------

fn golden_dir(backend: XDialogBackend) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/visual_references/egui").join(theme_name(backend))
}

fn seeding() -> bool {
    std::env::var("XDIALOG_VISUAL_SEED").is_ok_and(|v| v == "1")
}

/// Compare `theme`'s golden variants; returns (compared, skipped) names.
fn check_goldens(backend: XDialogBackend, variants: &[Variant]) -> (Vec<String>, Vec<String>) {
    let (dir, theme) = (golden_dir(backend), theme_name(backend));
    let (mut done, mut skipped, mut failures) = (Vec::new(), Vec::new(), Vec::new());
    for v in variants.iter().filter(|v| v.golden) {
        for f in run_variant(backend, v).unwrap_or_else(|e| panic!("{e}")) {
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
    // A golden that was never added must not pass unnoticed in CI.
    assert!(skipped.is_empty() || std::env::var_os("CI").is_none(), "{theme}: goldens missing in CI");
    (done, skipped)
}

#[test]
fn ubuntu_goldens() {
    let (done, _) = check_goldens(XDialogBackend::Ubuntu, &ubuntu::variants());
    println!("ubuntu: {} goldens {}", done.len(), if seeding() { "seeded" } else { "compared" });
}

#[test]
fn fluent_goldens() {
    let Ok(meta) = std::fs::metadata(r"C:\Windows\Fonts\SegUIVar.ttf") else {
        println!("skipped: Segoe UI Variable missing or a different version");
        return;
    };
    let id = format!("len={}", meta.len());
    let record = golden_dir(XDialogBackend::Fluent).join("FONT_ID");
    if seeding() {
        std::fs::create_dir_all(golden_dir(XDialogBackend::Fluent)).unwrap();
        std::fs::write(&record, format!("{id}\n")).unwrap();
    } else if std::fs::read_to_string(&record).map(|s| s.trim().to_owned()).ok().as_deref() != Some(id.as_str()) {
        println!("skipped: Segoe UI Variable missing or a different version");
        return;
    }
    let (done, _) = check_goldens(XDialogBackend::Fluent, &fluent::variants());
    println!("fluent: {} goldens {}", done.len(), if seeding() { "seeded" } else { "compared" });
}
