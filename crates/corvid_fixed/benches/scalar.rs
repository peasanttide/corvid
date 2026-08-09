//! This crate's scalar arithmetic against the platform's own.
//!
//! ```sh
//! cargo bench -p corvid_fixed --bench scalar
//! ```
//!
//! The tangent and the arcsine are here rather than in `trig` because what they
//! cost is a division and a CORDIC rather than the octant fold the sine shares.
//! The two roots and the multiply are the operations everything above is built
//! out of, so their ratios set the floor for every other row in the workspace.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "a phase is reinterpreted as the operand each row wants, which is what lets one input table serve them all"
)]

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

use corvid_fixed::{Angle16, I2F30, I24F8, Pitch16, Signed16};

mod common;

use common::{Inputs, SAMPLES};

/// The tangent, which is a sine over a cosine and a division that has to be
/// total at the poles.
fn tangent(c: &mut Criterion) {
    let input = Inputs::new();
    let mut group = c.benchmark_group("tan");
    group.throughput(Throughput::Elements(SAMPLES));

    group.bench_function("f64::tan", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &r in &input.radians {
                acc = acc.wrapping_add(black_box(r).tan().to_bits());
            }
            acc
        });
    });
    group.bench_function("Angle16::tan", |b| {
        b.iter(|| {
            let mut acc = 0_i64;
            for &p in &input.phases {
                let angle = Angle16::from_bits(black_box(p) as u16);
                acc = acc.wrapping_add(i64::from(angle.tan().to_bits()));
            }
            acc
        });
    });
    group.finish();
}

/// The inverse trigonometry, which is the other CORDIC.
fn arcsine(c: &mut Criterion) {
    let input = Inputs::new();
    let mut group = c.benchmark_group("asin");
    group.throughput(Throughput::Elements(SAMPLES));

    group.bench_function("f64::asin", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &r in &input.radians {
                acc = acc.wrapping_add((black_box(r) / 8.0).asin().to_bits());
            }
            acc
        });
    });
    group.bench_function("Pitch16::asin", |b| {
        b.iter(|| {
            let mut acc = 0_i64;
            for &p in &input.phases {
                let value = Signed16::from_bits(black_box(p) as i16);
                acc = acc.wrapping_add(i64::from(Pitch16::asin(value).to_bits()));
            }
            acc
        });
    });
    group.bench_function("Angle16::acos", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &p in &input.phases {
                let value = Signed16::from_bits(black_box(p) as i16);
                acc = acc.wrapping_add(u64::from(Angle16::acos(value).to_bits()));
            }
            acc
        });
    });
    group.finish();
}

/// The square root, over the whole positive range.
fn square_root(c: &mut Criterion) {
    let input = Inputs::new();
    let mut group = c.benchmark_group("sqrt");
    group.throughput(Throughput::Elements(SAMPLES));

    group.bench_function("f64::sqrt", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &r in &input.radians {
                acc = acc.wrapping_add(black_box(r).abs().sqrt().to_bits());
            }
            acc
        });
    });
    group.bench_function("I24F8::sqrt", |b| {
        b.iter(|| {
            let mut acc = 0_i64;
            for &p in &input.positives {
                let value = I24F8::from_bits(black_box(p));
                acc = acc.wrapping_add(i64::from(value.sqrt().to_bits()));
            }
            acc
        });
    });
    group.finish();
}

/// The reciprocal square root, in both tiers and against the two-step form it
/// replaces.
fn reciprocal_square_root(c: &mut Criterion) {
    let input = Inputs::new();
    let mut group = c.benchmark_group("rsqrt");
    group.throughput(Throughput::Elements(SAMPLES));

    group.bench_function("f64 1.0 / sqrt", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &r in &input.radians {
                let value = black_box(r).abs() + 1.0;
                acc = acc.wrapping_add((1.0 / value.sqrt()).to_bits());
            }
            acc
        });
    });
    group.bench_function("I2F30::rsqrt", |b| {
        b.iter(|| {
            let mut acc = 0_i64;
            for &p in &input.positives {
                let value = I2F30::from_bits(black_box(p));
                acc = acc.wrapping_add(i64::from(value.rsqrt().to_bits()));
            }
            acc
        });
    });
    group.bench_function("I2F30::rsqrt_fast", |b| {
        b.iter(|| {
            let mut acc = 0_i64;
            for &p in &input.positives {
                let value = I2F30::from_bits(black_box(p));
                acc = acc.wrapping_add(i64::from(value.rsqrt_fast().to_bits()));
            }
            acc
        });
    });
    group.bench_function("I2F30::sqrt then recip", |b| {
        b.iter(|| {
            let mut acc = 0_i64;
            for &p in &input.positives {
                let value = I2F30::from_bits(black_box(p));
                acc = acc.wrapping_add(i64::from(value.sqrt().recip().to_bits()));
            }
            acc
        });
    });
    group.finish();
}

/// The multiply every operation above is built out of, and the two composed
/// forms that round once where a naive pair would round twice.
fn multiplication(c: &mut Criterion) {
    let input = Inputs::new();
    let mut group = c.benchmark_group("mul");
    group.throughput(Throughput::Elements(SAMPLES));

    group.bench_function("f64 multiply", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &r in &input.radians {
                acc = acc.wrapping_add((black_box(r) * black_box(r)).to_bits());
            }
            acc
        });
    });
    group.bench_function("I24F8::saturating_mul", |b| {
        b.iter(|| {
            let mut acc = 0_i64;
            for &p in &input.positives {
                let value = I24F8::from_bits(black_box(p));
                acc = acc.wrapping_add(i64::from(value.saturating_mul(value).to_bits()));
            }
            acc
        });
    });
    group.bench_function("I24F8::mul_add", |b| {
        b.iter(|| {
            let mut acc = 0_i64;
            for &p in &input.positives {
                let value = I24F8::from_bits(black_box(p));
                acc = acc.wrapping_add(i64::from(value.mul_add(value, value).to_bits()));
            }
            acc
        });
    });
    group.bench_function("I24F8::hypot", |b| {
        b.iter(|| {
            let mut acc = 0_i64;
            for &p in &input.positives {
                let value = I24F8::from_bits(black_box(p));
                acc = acc.wrapping_add(i64::from(value.hypot(value).to_bits()));
            }
            acc
        });
    });
    group.bench_function("Signed16::mul", |b| {
        b.iter(|| {
            let mut acc = 0_i64;
            for &p in &input.phases {
                let value = Signed16::from_bits(black_box(p) as i16);
                acc = acc.wrapping_add(i64::from(value.mul(value).to_bits()));
            }
            acc
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    tangent,
    arcsine,
    square_root,
    reciprocal_square_root,
    multiplication
);
criterion_main!(benches);
