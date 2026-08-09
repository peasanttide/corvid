//! `Basis` against `Versor`, and both against an `f32` baseline.
//!
//! ```sh
//! cargo bench -p corvid_rotation
//! ```
//!
//! The `f32` baseline is what says whether determinism costs anything, so every
//! group opens with one and the ratio to it is the number to read. Each row
//! walks a fixed table of pseudo-random rotations, which is what stops branch
//! prediction from cheating.
//!
//! Every component of every result is folded into the accumulator, and that is
//! load-bearing rather than tidy. These operations are `#[inline]` and return a
//! plain struct, so LLVM scalarizes the result and deletes the work behind
//! whichever components nothing reads -- two of three for a rotated point, eight
//! of nine for a composed basis. `black_box` on the *inputs* does not stop it.
//! Measured understatement before the folds went in: 4.8x on `rotate_fine`,
//! 6.3x on `Basis::compose`, 6.9x on `Versor::compose`.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    reason = "the input table is built by reinterpreting raw bit patterns, which is what makes it a spread rather than a handful of round numbers"
)]
#![allow(
    clippy::many_single_char_names,
    clippy::suboptimal_flops,
    reason = "x, y, z and w are the names this subject matter uses, and the f32 baseline is written as plain arithmetic so it stays independent of the implementation"
)]

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

use corvid_fixed::{I2F30, I16F16};
use corvid_rotation::{Basis, FineRotation, Rotation, Versor};
use corvid_vector::FinePoint;

/// Rotations per row.
const SAMPLES: u64 = 4096;

/// A deterministic xorshift64\*, so every run measures the same work.
struct Rng(u64);

impl Rng {
    /// The next value in the sequence.
    const fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// The next value in `[0, 1)`.
    fn next_unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1_u64 << 53) as f64
    }
}

/// A uniformly distributed unit quaternion, by Shoemake's method.
fn random_quaternion(rng: &mut Rng) -> [f64; 4] {
    let (u1, u2, u3) = (rng.next_unit(), rng.next_unit(), rng.next_unit());
    let (r1, r2) = ((1.0 - u1).sqrt(), u1.sqrt());
    let (t1, t2) = (core::f64::consts::TAU * u2, core::f64::consts::TAU * u3);
    [r1 * t1.sin(), r1 * t1.cos(), r2 * t2.sin(), r2 * t2.cos()]
}

/// An `f32` 3x3 matrix, the baseline this crate is measured against.
type Matrix32 = [[f32; 3]; 3];

/// The `f32` matrix of a quaternion.
fn matrix_f32(q: [f64; 4]) -> Matrix32 {
    let (x, y, z, w) = (q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32);
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

/// Every table a row runs over, drawn once.
struct Inputs {
    /// The rotations, as versors.
    versors: Vec<Versor>,
    /// The same rotations as matrices.
    bases: Vec<Basis>,
    /// The same rotations as `f32` matrices, for the baselines.
    matrices: Vec<Matrix32>,
    /// The same rotations packed into 32 bits.
    packed: Vec<Rotation>,
    /// The same rotations packed into 64 bits.
    fine: Vec<FineRotation>,
    /// Near-field points to rotate.
    points: Vec<FinePoint>,
    /// The same points as `f32` triples.
    points_f32: Vec<[f32; 3]>,
}

impl Inputs {
    /// Draws every table.
    fn new() -> Self {
        let mut rng = Rng(0x2024_c0de_face_b00c);
        let quaternions: Vec<[f64; 4]> =
            (0..SAMPLES).map(|_| random_quaternion(&mut rng)).collect();
        let versors: Vec<Versor> = quaternions
            .iter()
            .map(|&q| {
                let component = I2F30::from_f64;
                let (x, y) = (component(q[0]), component(q[1]));
                let (z, w) = (component(q[2]), component(q[3]));
                Versor::from_xyzw(x, y, z, w).unwrap_or(Versor::IDENTITY)
            })
            .collect();
        let bases = versors.iter().map(|q| q.to_basis()).collect();
        let matrices = quaternions.iter().map(|&q| matrix_f32(q)).collect();
        let packed = versors.iter().map(|&q| Rotation::from_versor(q)).collect();
        let fine = versors
            .iter()
            .map(|&q| FineRotation::from_versor(q))
            .collect();
        let points: Vec<FinePoint> = (0..SAMPLES)
            .map(|_| {
                let mut component = || I16F16::from_f64(rng.next_unit() * 2000.0 - 1000.0);
                FinePoint::new(component(), component(), component())
            })
            .collect();
        let points_f32 = points
            .iter()
            .map(|p| [p.x().to_f32(), p.y().to_f32(), p.z().to_f32()])
            .collect();
        Self {
            versors,
            bases,
            matrices,
            packed,
            fine,
            points,
            points_f32,
        }
    }
}

/// Folds every component of a rotated point into the accumulator.
const fn fold_point(acc: u64, p: FinePoint) -> u64 {
    let [x, y, z] = p.to_array();
    acc.wrapping_add(x.to_bits() as u64)
        .wrapping_add(y.to_bits() as u64)
        .wrapping_add(z.to_bits() as u64)
}

/// Folds all nine entries of a basis into the accumulator.
fn fold_basis(acc: u64, m: Basis) -> u64 {
    let mut acc = acc;
    for row in m.to_rows() {
        for entry in row {
            acc = acc.wrapping_add(entry.to_bits() as u64);
        }
    }
    acc
}

/// Folds all four components of a versor into the accumulator.
fn fold_versor(acc: u64, q: Versor) -> u64 {
    let mut acc = acc;
    for component in q.to_xyzw() {
        acc = acc.wrapping_add(component.to_bits() as u64);
    }
    acc
}

/// Applying a rotation to a point, which is what a frame does thousands of
/// times.
fn rotating(c: &mut Criterion) {
    let input = Inputs::new();
    let mut group = c.benchmark_group("rotate");
    group.throughput(Throughput::Elements(SAMPLES));

    group.bench_function("f32 matrix", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for (m, v) in input.matrices.iter().zip(input.points_f32.iter()) {
                let (m, v) = (black_box(m), black_box(v));
                let x = m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2];
                let y = m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2];
                let z = m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2];
                acc = acc
                    .wrapping_add(u64::from(x.to_bits()))
                    .wrapping_add(u64::from(y.to_bits()))
                    .wrapping_add(u64::from(z.to_bits()));
            }
            acc
        });
    });
    group.bench_function("Basis::rotate_fine", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for (m, v) in input.bases.iter().zip(input.points.iter()) {
                acc = fold_point(acc, black_box(m).rotate_fine(*black_box(v)));
            }
            acc
        });
    });
    group.bench_function("Basis::unrotate_fine", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for (m, v) in input.bases.iter().zip(input.points.iter()) {
                acc = fold_point(acc, black_box(m).unrotate_fine(*black_box(v)));
            }
            acc
        });
    });
    group.bench_function("Versor::rotate_fine", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for (q, v) in input.versors.iter().zip(input.points.iter()) {
                acc = fold_point(acc, black_box(q).rotate_fine(*black_box(v)));
            }
            acc
        });
    });
    group.finish();
}

/// Composing two rotations, which is what a hierarchy does once per node.
fn composing(c: &mut Criterion) {
    let input = Inputs::new();
    let mut group = c.benchmark_group("compose");
    group.throughput(Throughput::Elements(SAMPLES));

    group.bench_function("f32 matrix", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for (a, other) in input.matrices.iter().zip(input.matrices.iter().rev()) {
                let (a, other) = (black_box(a), black_box(other));
                for row in a {
                    let columns = other[0].iter().zip(other[1].iter().zip(other[2].iter()));
                    for (b0, (b1, b2)) in columns {
                        let e = row[0] * b0 + row[1] * b1 + row[2] * b2;
                        acc = acc.wrapping_add(u64::from(e.to_bits()));
                    }
                }
            }
            acc
        });
    });
    group.bench_function("Basis::compose", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for (a, other) in input.bases.iter().zip(input.bases.iter().rev()) {
                acc = fold_basis(acc, black_box(a).compose(*black_box(other)));
            }
            acc
        });
    });
    group.bench_function("Versor::compose", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for (a, other) in input.versors.iter().zip(input.versors.iter().rev()) {
                acc = fold_versor(acc, black_box(a).compose(*black_box(other)));
            }
            acc
        });
    });
    group.finish();
}

/// Packing for the wire and unpacking on the other side.
fn packing(c: &mut Criterion) {
    let input = Inputs::new();
    let mut group = c.benchmark_group("pack");
    group.throughput(Throughput::Elements(SAMPLES));

    group.bench_function("Versor::to_basis", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for q in &input.versors {
                acc = fold_basis(acc, black_box(q).to_basis());
            }
            acc
        });
    });
    group.bench_function("Rotation::from_versor", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for q in &input.versors {
                let packed = Rotation::from_versor(*black_box(q));
                acc = acc.wrapping_add(u64::from(packed.to_bits()));
            }
            acc
        });
    });
    group.bench_function("Rotation::to_versor", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for r in &input.packed {
                acc = fold_versor(acc, black_box(r).to_versor());
            }
            acc
        });
    });
    group.bench_function("FineRotation::from_versor", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for q in &input.versors {
                acc = acc.wrapping_add(FineRotation::from_versor(*black_box(q)).to_bits());
            }
            acc
        });
    });
    group.bench_function("FineRotation::to_versor", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for r in &input.fine {
                acc = fold_versor(acc, black_box(r).to_versor());
            }
            acc
        });
    });
    group.bench_function("Rotation::to_basis", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for r in &input.packed {
                acc = fold_basis(acc, black_box(r).to_basis());
            }
            acc
        });
    });
    group.finish();
}

criterion_group!(benches, rotating, composing, packing);
criterion_main!(benches);
