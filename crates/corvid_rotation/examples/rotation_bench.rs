//! Times `Basis` against `Versor`, and both against an `f32` baseline.
//!
//! ```sh
//! cargo run --release --example rotation_bench
//! ```
//!
//! The `f32` baseline is what tells us whether determinism costs anything.
//! Each case runs over a fixed table of pseudo-random inputs — so branch
//! prediction cannot cheat — and reports the best of several rounds. Results
//! are summed into a `black_box` so nothing is optimized away.

#![allow(
    clippy::print_stdout,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::too_many_lines,
    reason = "a benchmark prints numbers, and reaches into raw bit patterns to build its inputs"
)]
#![allow(
    clippy::many_single_char_names,
    clippy::suboptimal_flops,
    clippy::items_after_statements,
    clippy::missing_const_for_fn,
    reason = "x, y, z and w are the names this subject matter uses, and the f64 references are written as plain arithmetic so they stay independent of the implementation"
)]

use std::hint::black_box;
use std::time::Instant;

use corvid_fixed::I16F16;
use corvid_rotation::{Basis, FineRotation, Rotation, Versor};
use corvid_vector::FinePoint;

/// Inputs per round.
const SAMPLES: usize = 4096;

/// Rounds per case; the fastest is reported.
const ROUNDS: usize = 12;

/// Passes over the sample table per round.
const PASSES: usize = 8;

/// A deterministic xorshift64\*, so every run measures the same work.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn next_unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A uniformly distributed unit quaternion.
fn random_quaternion(rng: &mut Rng) -> [f64; 4] {
    let (u1, u2, u3) = (rng.next_unit(), rng.next_unit(), rng.next_unit());
    let (r1, r2) = ((1.0 - u1).sqrt(), u1.sqrt());
    let (t1, t2) = (core::f64::consts::TAU * u2, core::f64::consts::TAU * u3);
    [r1 * t1.sin(), r1 * t1.cos(), r2 * t2.sin(), r2 * t2.cos()]
}

/// An `f32` 3×3 matrix, the baseline this crate is measured against.
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

/// Times `body` and reports nanoseconds per operation.
fn bench(name: &str, baseline: Option<f64>, mut body: impl FnMut() -> u64) -> f64 {
    black_box(body());

    let mut best = f64::INFINITY;
    for _ in 0..ROUNDS {
        let start = Instant::now();
        let checksum = body();
        let elapsed = start.elapsed();
        black_box(checksum);
        best = best.min(elapsed.as_secs_f64() * 1e9 / (SAMPLES * PASSES) as f64);
    }

    match baseline {
        Some(reference) => println!("  {name:<34} {best:>8.2} ns   {:>6.2}x", best / reference),
        None => println!("  {name:<34} {best:>8.2} ns        --"),
    }
    best
}

/// Folds **every** component of a rotated point into the accumulator.
///
/// Consuming one component is not enough. These operations are `#[inline]` and
/// return a plain struct, so LLVM scalarizes the result and deletes the work
/// behind whichever components nothing reads — two of three here, eight of nine
/// for a `Basis::compose`. `black_box` on the *inputs* does not stop it, and
/// the `f32` baselines below are immune only because they compute one scalar to
/// begin with. Measured understatement before this was fixed: 4.8x on
/// `rotate_fine`, 6.3x on `Basis::compose`, 6.9x on `Versor::compose`.
fn fold_point(acc: u64, p: FinePoint) -> u64 {
    let [x, y, z] = p.to_array();
    acc.wrapping_add(x.to_bits() as u64)
        .wrapping_add(y.to_bits() as u64)
        .wrapping_add(z.to_bits() as u64)
}

/// Folds all nine entries of a basis into the accumulator. See [`fold_point`].
fn fold_basis(acc: u64, m: Basis) -> u64 {
    let mut acc = acc;
    for row in m.to_rows() {
        for entry in row {
            acc = acc.wrapping_add(entry.to_bits() as u64);
        }
    }
    acc
}

/// Folds all four components of a versor into the accumulator. See [`fold_point`].
fn fold_versor(acc: u64, q: Versor) -> u64 {
    let mut acc = acc;
    for component in q.to_xyzw() {
        acc = acc.wrapping_add(component.to_bits() as u64);
    }
    acc
}

fn main() {
    let mut rng = Rng(0x2024_c0de_face_b00c);
    let quaternions: Vec<[f64; 4]> = (0..SAMPLES).map(|_| random_quaternion(&mut rng)).collect();

    let versors: Vec<Versor> = quaternions
        .iter()
        .map(|&q| {
            let c = |v: f64| corvid_fixed::I2F30::from_f64(v);
            Versor::from_xyzw(c(q[0]), c(q[1]), c(q[2]), c(q[3])).unwrap_or(Versor::IDENTITY)
        })
        .collect();
    let bases: Vec<Basis> = versors.iter().map(|q| q.to_basis()).collect();
    let matrices: Vec<Matrix32> = quaternions.iter().map(|&q| matrix_f32(q)).collect();
    let packed: Vec<Rotation> = versors.iter().map(|&q| Rotation::from_versor(q)).collect();
    let fine: Vec<FineRotation> = versors
        .iter()
        .map(|&q| FineRotation::from_versor(q))
        .collect();

    let points: Vec<FinePoint> = (0..SAMPLES)
        .map(|_| {
            let mut c = || I16F16::from_f64(rng.next_unit() * 2000.0 - 1000.0);
            FinePoint::new(c(), c(), c())
        })
        .collect();
    let points_f32: Vec<[f32; 3]> = points
        .iter()
        .map(|p| [p.x().to_f32(), p.y().to_f32(), p.z().to_f32()])
        .collect();

    println!("\nrotating a point ({SAMPLES} inputs x {PASSES} passes, best of {ROUNDS})");
    let baseline = bench("f32 matrix", None, || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for (m, v) in matrices.iter().zip(points_f32.iter()) {
                let m = black_box(m);
                let v = black_box(v);
                // All three components, for the same reason `fold_point`
                // exists: a baseline that computes one third of the work is
                // not a baseline for the fixed-point call beside it.
                let x = m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2];
                let y = m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2];
                let z = m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2];
                acc = acc
                    .wrapping_add(u64::from(x.to_bits()))
                    .wrapping_add(u64::from(y.to_bits()))
                    .wrapping_add(u64::from(z.to_bits()));
            }
        }
        acc
    });
    bench("Basis::rotate_fine", Some(baseline), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for (m, v) in bases.iter().zip(points.iter()) {
                acc = fold_point(acc, black_box(m).rotate_fine(black_box(*v)));
            }
        }
        acc
    });
    bench("Basis::unrotate_fine", Some(baseline), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for (m, v) in bases.iter().zip(points.iter()) {
                acc = fold_point(acc, black_box(m).unrotate_fine(black_box(*v)));
            }
        }
        acc
    });
    bench("Versor::rotate_fine", Some(baseline), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for (q, v) in versors.iter().zip(points.iter()) {
                acc = fold_point(acc, black_box(q).rotate_fine(black_box(*v)));
            }
        }
        acc
    });

    println!("\ncomposing two rotations");
    let compose_baseline = bench("f32 matrix", None, || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for (a, b) in matrices.iter().zip(matrices.iter().rev()) {
                let (a, b) = (black_box(a), black_box(b));
                // The whole 3x3 product: one entry of nine is not a baseline
                // for `Basis::compose`, which produces all nine.
                for row in a {
                    for (b0, (b1, b2)) in b[0].iter().zip(b[1].iter().zip(b[2].iter())) {
                        let e = row[0] * b0 + row[1] * b1 + row[2] * b2;
                        acc = acc.wrapping_add(u64::from(e.to_bits()));
                    }
                }
            }
        }
        acc
    });
    bench("Basis::compose", Some(compose_baseline), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for (a, b) in bases.iter().zip(bases.iter().rev()) {
                acc = fold_basis(acc, black_box(a).compose(*black_box(b)));
            }
        }
        acc
    });
    bench("Versor::compose", Some(compose_baseline), || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for (a, b) in versors.iter().zip(versors.iter().rev()) {
                acc = fold_versor(acc, black_box(a).compose(*black_box(b)));
            }
        }
        acc
    });

    println!("\npacking and unpacking");
    let pack_baseline = bench("Versor -> Basis", None, || {
        let mut acc = 0_u64;
        for _ in 0..PASSES {
            for q in &versors {
                acc = fold_basis(acc, black_box(q).to_basis());
            }
        }
        acc
    });
    bench(
        "Rotation::from_versor (pack 32)",
        Some(pack_baseline),
        || {
            let mut acc = 0_u64;
            for _ in 0..PASSES {
                for q in &versors {
                    acc =
                        acc.wrapping_add(u64::from(Rotation::from_versor(*black_box(q)).to_bits()));
                }
            }
            acc
        },
    );
    bench(
        "Rotation::to_versor (unpack 32)",
        Some(pack_baseline),
        || {
            let mut acc = 0_u64;
            for _ in 0..PASSES {
                for r in &packed {
                    acc = fold_versor(acc, black_box(r).to_versor());
                }
            }
            acc
        },
    );
    bench(
        "FineRotation::from_versor (pack 64)",
        Some(pack_baseline),
        || {
            let mut acc = 0_u64;
            for _ in 0..PASSES {
                for q in &versors {
                    acc = acc.wrapping_add(FineRotation::from_versor(*black_box(q)).to_bits());
                }
            }
            acc
        },
    );
    bench(
        "FineRotation::to_versor (unpack 64)",
        Some(pack_baseline),
        || {
            let mut acc = 0_u64;
            for _ in 0..PASSES {
                for r in &fine {
                    acc = fold_versor(acc, black_box(r).to_versor());
                }
            }
            acc
        },
    );
    bench(
        "Rotation::to_basis (unpack + matrix)",
        Some(pack_baseline),
        || {
            let mut acc = 0_u64;
            for _ in 0..PASSES {
                for r in &packed {
                    acc = fold_basis(acc, black_box(r).to_basis());
                }
            }
            acc
        },
    );
    println!();
}
