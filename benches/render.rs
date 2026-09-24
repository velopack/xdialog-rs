//! Offscreen frame render benchmark per theme (software rendering performance).
//!
//! `cargo bench --bench render --features _test-hooks,linux-egui[,fluent-egui]`
//!
//! Each case measures one full frame through the real `Dialog` path (egui pass, tessellation,
//! software raster into memory, RGBA copy): a static frame, a frame during the hover fade, an
//! indeterminate progress frame, a 2× HiDPI frame, and dialog construction (fonts + measure pass).

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use xdialog::__test::{HostEvent, OffscreenDialog, TestAppearance, TestKind, TestProgress};
use xdialog::{XDialogIcon, XDialogOptions};

fn themes() -> Vec<&'static str> {
    let mut t = vec!["linux"];
    if cfg!(feature = "fluent-egui") {
        t.push("fluent");
    }
    t
}

fn message() -> XDialogOptions {
    XDialogOptions { title: "Update Available".into(),
                     main_instruction: "New version available".into(),
                     message: "Would you like to update to the new version now?".into(),
                     icon: XDialogIcon::Warning,
                     buttons: vec!["No".into(), "Yes".into()] }
}

fn dialog(theme: &str, ppp: f32, kind: TestKind) -> OffscreenDialog {
    let mut o = message();
    if matches!(kind, TestKind::Progress) {
        o.buttons = vec!["Cancel".into()];
    }
    OffscreenDialog::new(theme, TestAppearance::default(), ppp, kind, o).expect("offscreen dialog")
}

fn bench(c: &mut Criterion) {
    for theme in themes() {
        let mut g = c.benchmark_group(format!("render/{theme}"));

        // Static frame (nothing animating): time advances, output identical.
        let mut d = dialog(theme, 1.0, TestKind::Message);
        let mut t = 1.0;
        g.bench_function(BenchmarkId::new("static", "1x"), |b| {
             b.iter(|| {
                  t += 0.016;
                  d.render_at(t)
              })
         });

        // HiDPI static frame.
        let mut d = dialog(theme, 2.0, TestKind::Message);
        let mut t = 1.0;
        g.bench_function(BenchmarkId::new("static", "2x"), |b| {
             b.iter(|| {
                  t += 0.016;
                  d.render_at(t)
              })
         });

        // Hover fade: alternate hover on/off every 10 frames so tweens are always running.
        let mut d = dialog(theme, 1.0, TestKind::Message);
        d.render_at(0.5);
        let (x, y) = d.button_centre_px(0).expect("button");
        let centre = HostEvent::CursorMoved { x, y };
        let away = HostEvent::CursorMoved { x: 2.0, y: 2.0 };
        let (mut t, mut n) = (1.0, 0u64);
        g.bench_function(BenchmarkId::new("hover_fade", "1x"), |b| {
             b.iter(|| {
                  n += 1;
                  if n % 10 == 0 {
                      d.event(if (n / 10) % 2 == 0 { centre } else { away });
                  }
                  t += 0.016;
                  d.render_at(t)
              })
         });

        // Indeterminate progress (repaints every frame).
        let mut d = dialog(theme, 1.0, TestKind::Progress);
        d.set_progress(TestProgress::Indeterminate);
        let mut t = 1.0;
        g.bench_function(BenchmarkId::new("indeterminate", "1x"), |b| {
             b.iter(|| {
                  t += 0.016;
                  d.render_at(t)
              })
         });

        // Construction: context, fonts, measure pass (no render).
        g.bench_function(BenchmarkId::new("new", "1x"), |b| b.iter(|| dialog(theme, 1.0, TestKind::Message)));
        g.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30).measurement_time(std::time::Duration::from_secs(3));
    targets = bench
}
criterion_main!(benches);
