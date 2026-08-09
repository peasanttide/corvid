//! This crate's trigonometry against the platform's own.
//!
//! ```sh
//! cargo bench -p corvid_fixed --bench trig
//! ```
//!
//! Every group opens with the `f64` row, so the number that matters is the
//! ratio between it and the rows below rather than either time on its own: an
//! integer sine that is bit-identical everywhere is worth a constant factor,
//! and the factor is what this reports.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a phase is read as an angle of each width in turn, which is the comparison these rows exist to make"
)]

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

use corvid_fixed::{Angle8, Angle16, Angle32};

mod common;

use common::{Inputs, SAMPLES};

/// The sine, at all three widths and in both accuracy tiers.
fn sine(c: &mut Criterion) {
    let input = Inputs::new();
    let mut group = c.benchmark_group("sin");
    group.throughput(Throughput::Elements(SAMPLES));

    group.bench_function("f64::sin", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &r in &input.radians {
                acc = acc.wrapping_add(black_box(r).sin().to_bits());
            }
            acc
        });
    });
    group.bench_function("f32::sin", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &r in &input.radians32 {
                acc = acc.wrapping_add(u64::from(black_box(r).sin().to_bits()));
            }
            acc
        });
    });
    group.bench_function("Angle32::sin", |b| {
        b.iter(|| {
            let mut acc = 0_i64;
            for &p in &input.phases {
                let angle = Angle32::from_bits(black_box(p));
                acc = acc.wrapping_add(i64::from(angle.sin().to_bits()));
            }
            acc
        });
    });
    group.bench_function("Angle16::sin", |b| {
        b.iter(|| {
            let mut acc = 0_i64;
            for &p in &input.phases {
                let angle = Angle16::from_bits(black_box(p) as u16);
                acc = acc.wrapping_add(i64::from(angle.sin().to_bits()));
            }
            acc
        });
    });
    group.bench_function("Angle8::sin", |b| {
        b.iter(|| {
            let mut acc = 0_i64;
            for &p in &input.phases {
                let angle = Angle8::from_bits(black_box(p) as u8);
                acc = acc.wrapping_add(i64::from(angle.sin().to_bits()));
            }
            acc
        });
    });
    group.bench_function("Angle16::sin_fast", |b| {
        b.iter(|| {
            let mut acc = 0_i64;
            for &p in &input.phases {
                let angle = Angle16::from_bits(black_box(p) as u16);
                acc = acc.wrapping_add(i64::from(angle.sin_fast().to_bits()));
            }
            acc
        });
    });
    group.finish();
}

/// Both together, which is one octant fold rather than two.
fn sine_and_cosine(c: &mut Criterion) {
    let input = Inputs::new();
    let mut group = c.benchmark_group("sin_cos");
    group.throughput(Throughput::Elements(SAMPLES));

    group.bench_function("f64::sin_cos", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &r in &input.radians {
                let (s, cos) = black_box(r).sin_cos();
                acc = acc.wrapping_add(s.to_bits() ^ cos.to_bits());
            }
            acc
        });
    });
    group.bench_function("Angle16::sin_cos", |b| {
        b.iter(|| {
            let mut acc = 0_i64;
            for &p in &input.phases {
                let (s, cos) = Angle16::from_bits(black_box(p) as u16).sin_cos();
                acc = acc.wrapping_add(i64::from(s.to_bits() ^ cos.to_bits()));
            }
            acc
        });
    });
    group.finish();
}

/// The arctangent, which is CORDIC in the exact tier and a polynomial in the
/// fast one.
fn arctangent(c: &mut Criterion) {
    let input = Inputs::new();
    let mut group = c.benchmark_group("atan2");
    group.throughput(Throughput::Elements(SAMPLES));

    group.bench_function("f64::atan2", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &(y, x) in &input.coords_f {
                acc = acc.wrapping_add(black_box(y).atan2(black_box(x)).to_bits());
            }
            acc
        });
    });
    group.bench_function("Angle32::atan2", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &(y, x) in &input.coords {
                let angle = Angle32::atan2(black_box(y), black_box(x));
                acc = acc.wrapping_add(u64::from(angle.to_bits()));
            }
            acc
        });
    });
    group.bench_function("Angle16::atan2", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &(y, x) in &input.coords {
                let angle = Angle16::atan2(black_box(y), black_box(x));
                acc = acc.wrapping_add(u64::from(angle.to_bits()));
            }
            acc
        });
    });
    group.bench_function("Angle16::atan2_fast", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &(y, x) in &input.coords32 {
                let angle = Angle16::atan2_fast(black_box(y), black_box(x));
                acc = acc.wrapping_add(u64::from(angle.to_bits()));
            }
            acc
        });
    });
    group.finish();
}

criterion_group!(benches, sine, sine_and_cosine, arctangent);
criterion_main!(benches);
