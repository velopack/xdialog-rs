//! Benchmark for the present-time RGBA→0RGB conversion.
//!
//! Compares the scalar reference against the `multiversion` (auto-vectorized, runtime-dispatched)
//! implementation over a buffer the size of a large HiDPI dialog. Run with:
//!
//! ```sh
//! cargo bench --bench convert
//! ```
//!
//! Because `xdialog::pixels` is portable, this exercises whatever SIMD the host CPU has — NEON on
//! Apple Silicon, AVX/SSE on x86 — so the speedup is measured on the machine you build on.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use xdialog::pixels::{rgba_to_argb, rgba_to_argb_scalar};

/// ~1200×1000 px — a large dialog at 2× scale.
const WIDTH: usize = 1200;
const HEIGHT: usize = 1000;
const PIXELS: usize = WIDTH * HEIGHT;

fn make_src() -> Vec<u8> {
    // Deterministic, varied bytes so the optimizer can't fold the work away.
    (0..PIXELS * 4).map(|i| (i * 31 + 7) as u8).collect()
}

fn bench_convert(c: &mut Criterion) {
    let src = make_src();

    let mut group = c.benchmark_group("rgba_to_argb");
    group.throughput(Throughput::Bytes((PIXELS * 4) as u64));

    group.bench_function("scalar", |b| {
        b.iter_batched_ref(
            || vec![0u32; PIXELS],
            |dst| rgba_to_argb_scalar(&src, dst),
            BatchSize::LargeInput,
        );
    });

    group.bench_function("multiversion", |b| {
        b.iter_batched_ref(
            || vec![0u32; PIXELS],
            |dst| rgba_to_argb(&src, dst),
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_convert);
criterion_main!(benches);
