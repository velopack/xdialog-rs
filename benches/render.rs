//! Offscreen frame render benchmark per drawn theme (both macOS styles), through this platform's
//! drawing backend (`xdialog::__test::RENDERER`: Direct2D on Windows, CoreGraphics on macOS,
//! software elsewhere).
//!
//! `cargo bench --bench render --features _test-hooks`
//!
//! Each case measures one full frame through the real `Dialog` path (theme pass, drawing into
//! memory, RGBA copy): a static frame, a frame during the hover fade, an indeterminate progress
//! frame, a 2× HiDPI frame, and dialog construction (fonts + measure pass). Group names stay
//! stable for `--save-baseline` / `--baseline`.

use criterion::measurement::WallTime;
use criterion::{criterion_group, criterion_main, BenchmarkGroup, BenchmarkId, Criterion};
use xdialog::__test::{Event, OffscreenDialog, Point, TestAppearance, TestKind, TestMacStyle, TestProgress};
use xdialog::{XDialogBackend, XDialogIcon, XDialogOptions};

fn message() -> XDialogOptions {
    XDialogOptions { title: "Update Available".into(),
                     main_instruction: "New version available".into(),
                     message: "Would you like to update to the new version now?".into(),
                     icon: XDialogIcon::Warning,
                     buttons: vec!["No".into(), "Yes".into()],
                     ..Default::default() }
}

fn dialog(backend: XDialogBackend, style: TestMacStyle, ppp: f64, kind: TestKind) -> OffscreenDialog {
    let mut o = message();
    if matches!(kind, TestKind::Progress) {
        o.buttons = vec!["Cancel".into()];
    }
    OffscreenDialog::with_mac_style(backend, style, TestAppearance::default(), ppp, kind, o)
}

/// One frame per iteration, the dialog clock advancing at 60 Hz.
fn frames(g: &mut BenchmarkGroup<WallTime>, id: BenchmarkId, mut d: OffscreenDialog) {
    let mut t = 1.0;
    g.bench_function(id, |b| {
         b.iter(|| {
              t += 0.016;
              d.render_at(t)
          })
     });
}

fn bench(c: &mut Criterion) {
    let looks = [(XDialogBackend::Ubuntu, TestMacStyle::Legacy, "ubuntu"),
                 (XDialogBackend::Fluent, TestMacStyle::Legacy, "fluent"),
                 (XDialogBackend::MacOS, TestMacStyle::Legacy, "macos"),
                 (XDialogBackend::MacOS, TestMacStyle::Tahoe, "macos_tahoe")];
    for (backend, style, name) in looks {
        let open = |ppp, kind| dialog(backend, style, ppp, kind);
        let mut g = c.benchmark_group(format!("render/{name}"));

        // Static frame (nothing animating): time advances, output identical.
        frames(&mut g, BenchmarkId::new("static", "1x"), open(1.0, TestKind::Message));
        frames(&mut g, BenchmarkId::new("static", "2x"), open(2.0, TestKind::Message));

        // Hover fade: alternate hover on/off every 10 frames so tweens are always running.
        let mut d = open(1.0, TestKind::Message);
        d.render_at(0.5);
        let centre = d.button_centre(0).expect("button");
        let away = Point::new(2.0, 2.0);
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
        let mut d = open(1.0, TestKind::Progress);
        d.set_progress(TestProgress::Indeterminate);
        frames(&mut g, BenchmarkId::new("indeterminate", "1x"), d);

        // Construction: context, fonts, measure pass, open frame.
        g.bench_function(BenchmarkId::new("new", "1x"), |b| b.iter(|| open(1.0, TestKind::Message)));
        g.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30).measurement_time(std::time::Duration::from_secs(3));
    targets = bench
}
criterion_main!(benches);
