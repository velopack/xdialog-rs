//! Offscreen frame render benchmark per egui theme (software rendering performance).
//!
//! `cargo bench --bench render --features _test-hooks`
//!
//! Each case measures one full frame through the real `Dialog` path (egui pass, tessellation,
//! software raster into memory, RGBA copy): a static frame, a frame during the hover fade, an
//! indeterminate progress frame, a 2× HiDPI frame, and dialog construction (fonts + measure pass).

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use xdialog::__test::egui::{Event, Pos2};
use xdialog::__test::{OffscreenDialog, TestAppearance, TestKind, TestProgress};
use xdialog::{XDialogBackend, XDialogIcon, XDialogOptions};

fn message() -> XDialogOptions {
    XDialogOptions { title: "Update Available".into(),
                     main_instruction: "New version available".into(),
                     message: "Would you like to update to the new version now?".into(),
                     icon: XDialogIcon::Warning,
                     buttons: vec!["No".into(), "Yes".into()] }
}

fn dialog(backend: XDialogBackend, ppp: f32, kind: TestKind) -> OffscreenDialog {
    let mut o = message();
    if matches!(kind, TestKind::Progress) {
        o.buttons = vec!["Cancel".into()];
    }
    OffscreenDialog::new(backend, TestAppearance::default(), ppp, kind, o)
}

fn bench(c: &mut Criterion) {
    for backend in [XDialogBackend::Ubuntu, XDialogBackend::Fluent] {
        let mut g = c.benchmark_group(format!("render/{}", format!("{backend:?}").to_lowercase()));

        // Static frame (nothing animating): time advances, output identical.
        let mut d = dialog(backend, 1.0, TestKind::Message);
        let mut t = 1.0;
        g.bench_function(BenchmarkId::new("static", "1x"), |b| {
             b.iter(|| {
                  t += 0.016;
                  d.render_at(t)
              })
         });

        // HiDPI static frame.
        let mut d = dialog(backend, 2.0, TestKind::Message);
        let mut t = 1.0;
        g.bench_function(BenchmarkId::new("static", "2x"), |b| {
             b.iter(|| {
                  t += 0.016;
                  d.render_at(t)
              })
         });

        // Hover fade: alternate hover on/off every 10 frames so tweens are always running.
        let mut d = dialog(backend, 1.0, TestKind::Message);
        d.render_at(0.5);
        let centre = d.button_centre(0).expect("button");
        let away = Pos2::new(2.0, 2.0);
        let (mut t, mut n) = (1.0, 0u64);
        g.bench_function(BenchmarkId::new("hover_fade", "1x"), |b| {
             b.iter(|| {
                  n += 1;
                  if n % 10 == 0 {
                      d.event(Event::PointerMoved(if (n / 10) % 2 == 0 { centre } else { away }));
                  }
                  t += 0.016;
                  d.render_at(t)
              })
         });

        // Indeterminate progress (repaints every frame).
        let mut d = dialog(backend, 1.0, TestKind::Progress);
        d.set_progress(TestProgress::Indeterminate);
        let mut t = 1.0;
        g.bench_function(BenchmarkId::new("indeterminate", "1x"), |b| {
             b.iter(|| {
                  t += 0.016;
                  d.render_at(t)
              })
         });

        // Construction: context, fonts, measure pass, open frame.
        g.bench_function(BenchmarkId::new("new", "1x"), |b| b.iter(|| dialog(backend, 1.0, TestKind::Message)));
        g.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30).measurement_time(std::time::Duration::from_secs(3));
    targets = bench
}
criterion_main!(benches);
