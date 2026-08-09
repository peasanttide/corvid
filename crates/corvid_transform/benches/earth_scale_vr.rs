//! The target scenario: 10,000 objects converted world->eye at 90 Hz, with the
//! camera 6371 km from the origin.
//!
//! ```sh
//! cargo bench -p corvid_transform
//! ```
//!
//! Criterion reports per-conversion time through the throughput line, and
//! 10,000 of them have to fit inside a 90 Hz frame's 11.1 ms alongside
//! everything else a frame does. What the rows quantify is the two decisions the
//! design rests on:
//!
//! - **Subtracting before narrowing.** The `i64` local path against the `i128`
//!   global path.
//! - **Hoisting the basis.** Decoding the packed rotation once per frame rather
//!   than once per point.
//!
//! Every component of every result is folded into the accumulator, and that is
//! load-bearing. These conversions are `#[inline]` and return a plain struct, so
//! LLVM scalarizes the result and deletes the work behind the two components
//! nothing reads; `black_box` on the *inputs* does not stop it. The
//! `to_fine_global` row happens to be immune, because its `Option` depends on
//! all three axes, so the rows compared against it were the only ones being
//! shortened. Measured understatement before the folds went in: 2.8x on the
//! `i64` path and 3.0x on the `i128` one, which turned a real win from hoisting
//! the basis into a much larger reported one.

#![allow(
    clippy::suboptimal_flops,
    reason = "the scene is placed by `distance + offset * extent`, which is how the sentence above reads; a `mul_add` would be faster and would say less"
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    reason = "the scene is built from raw bit patterns, and the folds narrow a component on purpose so that one accumulator serves every width"
)]

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

use corvid_transform::{
    Angle32, FinePoint, FineRotation, FineTransform, GlobalFinePoint, GlobalPoint, I48F16, Pitch32,
    Versor,
};

/// Objects converted per frame.
const OBJECTS: u64 = 10_000;

/// How far the camera sits from the origin: the earth's radius.
const CAMERA_DISTANCE: f64 = 6_371_000.0;

/// A deterministic xorshift64\*, so every run measures the same scene.
struct Rng(u64);

impl Rng {
    /// The next value in the sequence.
    const fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A value in `-1.0 ..= 1.0`.
    fn next_unit(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 / (1_u64 << 52) as f64) - 1.0
    }
}

/// Folds every component of a near-field point into the accumulator.
const fn fold_fine(acc: u64, p: FinePoint) -> u64 {
    let [x, y, z] = p.to_array();
    acc.wrapping_add(x.to_bits() as u32 as u64)
        .wrapping_add(y.to_bits() as u32 as u64)
        .wrapping_add(z.to_bits() as u32 as u64)
}

/// Folds every component of a world-scale point.
const fn fold_wide(acc: u64, p: GlobalFinePoint) -> u64 {
    let [x, y, z] = p.to_array();
    acc.wrapping_add(x.to_bits() as u64)
        .wrapping_add(y.to_bits() as u64)
        .wrapping_add(z.to_bits() as u64)
}

/// Folds every component of an object-scale point.
const fn fold_coarse(acc: u64, p: GlobalPoint) -> u64 {
    let [x, y, z] = p.to_array();
    acc.wrapping_add(x.to_bits() as u32 as u64)
        .wrapping_add(y.to_bits() as u32 as u64)
        .wrapping_add(z.to_bits() as u32 as u64)
}

/// The camera, and the objects scattered through a 20 km cube around it -- the
/// near field a renderer actually draws.
fn scene() -> (FineTransform, Vec<GlobalFinePoint>) {
    let camera = FineTransform::new(
        GlobalFinePoint::splat(I48F16::from_f64(CAMERA_DISTANCE)),
        FineRotation::from_versor(Versor::from_yaw_pitch_roll(
            Angle32::from_degrees(37.0),
            Pitch32::from_degrees(-12.0),
            Angle32::from_degrees(3.0),
        )),
    );
    let mut rng = Rng(0x2024_c0de_face_b00c);
    let objects = (0..OBJECTS)
        .map(|_| {
            let mut axis = || I48F16::from_f64(CAMERA_DISTANCE + rng.next_unit() * 10_000.0);
            GlobalFinePoint::new(axis(), axis(), axis())
        })
        .collect();
    (camera, objects)
}

/// World to eye, which is what every drawn object goes through once a frame.
fn world_to_eye(c: &mut Criterion) {
    let (camera, objects) = scene();
    let mut group = c.benchmark_group("world_to_eye");
    group.throughput(Throughput::Elements(OBJECTS));

    // The shipped path: decodes the packed rotation on every call.
    group.bench_function("FineTransform::to_fine_global", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &p in &objects {
                if let Some(local) = black_box(&camera).to_fine_global(black_box(p)) {
                    acc = fold_fine(acc, local);
                }
            }
            acc
        });
    });

    // The same work with the basis decoded once per frame, which is what the
    // documentation steers a hot loop toward.
    group.bench_function("hoisted basis, i64 local path", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            let basis = black_box(&camera).basis();
            let origin = camera.origin();
            for &p in &objects {
                if let Some(near) = black_box(p)
                    .checked_sub(origin)
                    .and_then(GlobalFinePoint::to_fine)
                {
                    acc = fold_fine(acc, basis.unrotate_fine(near));
                }
            }
            acc
        });
    });

    // Rotating at world width instead of subtracting first. The same answer, and
    // what the design exists to avoid.
    group.bench_function("hoisted basis, i128 global path", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            let basis = black_box(&camera).basis();
            let origin = camera.origin();
            for &p in &objects {
                let offset = black_box(p).sub(origin);
                acc = fold_wide(acc, basis.unrotate_global_fine(offset));
            }
            acc
        });
    });

    // The coarser output, for anything not drawn at eye resolution.
    group.bench_function("FineTransform::to_local_global", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &p in &objects {
                if let Some(local) = black_box(&camera).to_local_global(black_box(p)) {
                    acc = fold_coarse(acc, local);
                }
            }
            acc
        });
    });

    group.finish();
}

/// The return trip, over the objects that landed in the near field.
fn eye_to_world(c: &mut Criterion) {
    let (camera, objects) = scene();
    let near: Vec<FinePoint> = objects
        .iter()
        .filter_map(|&p| camera.to_fine_global(p))
        .collect();

    let mut group = c.benchmark_group("eye_to_world");
    group.throughput(Throughput::Elements(near.len() as u64));
    group.bench_function("FineTransform::to_world", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &v in &near {
                acc = fold_wide(acc, black_box(&camera).to_world(black_box(v)));
            }
            acc
        });
    });
    group.finish();
}

criterion_group!(benches, world_to_eye, eye_to_world);
criterion_main!(benches);
