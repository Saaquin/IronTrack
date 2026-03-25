use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("cts autoselect", |b| {
        b.iter(|| {
            egm96::egm96_altitude_offset(black_box(33.4818), black_box(-117.556));
        })
    });

    c.bench_function("cts harmonics", |b| {
        b.iter(|| {
            egm96::egm96_compute_altitude_offset(black_box(33.4818), black_box(-117.556));
        })
    });

    #[cfg(feature = "raster_15_min")]
    c.bench_function("cts 15 min", |b| {
        b.iter(|| {
            egm96::egm96_raster_15_min_altitude_offset(black_box(33.4818), black_box(-117.556));
        })
    });

    #[cfg(feature = "raster_5_min")]
    c.bench_function("cts 5 min", |b| {
        b.iter(|| {
            egm96::egm96_raster_5_min_altitude_offset(black_box(33.4818), black_box(-117.556));
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
