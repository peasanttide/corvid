//! Reproduces the codec error table in integer arithmetic.
//!
//! ```sh
//! cargo run --release --example rotation_quality
//! ```
//!
//! It measures what this crate actually computes, against an `f64` reference, by
//! the chord form `4·asin(chord/2)` — never `2·acos(|q1·q2|)`, whose noise
//! floor sits above `FineRotation`'s whole error budget.
//!
//! The rejected alternatives are implemented here as private functions rather
//! than in the public API, so the codec choice stays measured rather than
//! asserted without carrying dead weight in the library.

#![allow(
    clippy::print_stdout,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    reason = "an example prints numbers, and reaches into raw bit patterns to build its inputs"
)]
#![allow(
    clippy::many_single_char_names,
    clippy::suboptimal_flops,
    clippy::items_after_statements,
    clippy::missing_const_for_fn,
    clippy::needless_range_loop,
    reason = "x, y, z and w are the names this subject matter uses, and the f64 references are written as plain arithmetic so they stay independent of the implementation"
)]

use corvid_rotation::{FineRotation, Rotation, Versor};

/// Samples per codec.
const SAMPLES: u32 = 200_000;

/// A deterministic xorshift64\*, so every run measures the same rotations.
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

/// A uniformly distributed unit quaternion, by Shoemake's subgroup algorithm.
fn random_quaternion(rng: &mut Rng) -> [f64; 4] {
    let (u1, u2, u3) = (rng.next_unit(), rng.next_unit(), rng.next_unit());
    let (r1, r2) = ((1.0 - u1).sqrt(), u1.sqrt());
    let (t1, t2) = (core::f64::consts::TAU * u2, core::f64::consts::TAU * u3);
    [r1 * t1.sin(), r1 * t1.cos(), r2 * t2.sin(), r2 * t2.cos()]
}

/// The angle between two quaternions in degrees, by the chord form.
fn angle_degrees(a: [f64; 4], b: [f64; 4]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(p, q)| p * q).sum();
    let b = if dot < 0.0 {
        [-b[0], -b[1], -b[2], -b[3]]
    } else {
        b
    };
    let chord: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(p, q)| (p - q) * (p - q))
        .sum::<f64>()
        .sqrt();
    4.0 * (chord / 2.0).clamp(-1.0, 1.0).asin().to_degrees()
}

/// This crate's `Versor` nearest an `f64` quaternion.
fn to_versor(q: [f64; 4]) -> Versor {
    let c = |v: f64| corvid_fixed::I2F30::from_f64(v);
    Versor::from_xyzw(c(q[0]), c(q[1]), c(q[2]), c(q[3])).unwrap_or(Versor::IDENTITY)
}

/// A `Versor`'s components as `f64`.
fn from_versor(q: Versor) -> [f64; 4] {
    let [x, y, z, w] = q.to_xyzw();
    [x.to_f64(), y.to_f64(), z.to_f64(), w.to_f64()]
}

/// Round-trips through the crate's 32-bit codec.
fn gibbs_linear(q: [f64; 4]) -> [f64; 4] {
    from_versor(Rotation::from_versor(to_versor(q)).to_versor())
}

/// Round-trips through the crate's 64-bit codec.
fn fine_quaternion(q: [f64; 4]) -> [f64; 4] {
    from_versor(FineRotation::from_versor(to_versor(q)).to_versor())
}

/// The smallest-three baseline at 2+10+10+10, for comparison only.
///
/// Drops the largest component and stores the other three directly rather than
/// dividing by it. Cheaper still to decode, and it misses the 1/5° budget —
/// which is the whole reason the Gibbs form is the one that ships.
fn smallest_three(q: [f64; 4]) -> [f64; 4] {
    let mut chart = 0;
    for i in 1..4 {
        if q[i].abs() > q[chart].abs() {
            chart = i;
        }
    }
    let signed = if q[chart] < 0.0 {
        [-q[0], -q[1], -q[2], -q[3]]
    } else {
        q
    };

    // The other three lie in [-1/sqrt(2), 1/sqrt(2)].
    const LIMIT: f64 = core::f64::consts::FRAC_1_SQRT_2;
    const FIELD_MAX: f64 = 511.0;
    let mut out = [0.0f64; 4];
    let mut sum = 0.0;
    for i in 0..4 {
        if i != chart {
            let quantized = (signed[i] / LIMIT * FIELD_MAX)
                .round()
                .clamp(-FIELD_MAX, FIELD_MAX);
            out[i] = quantized / FIELD_MAX * LIMIT;
            sum += out[i] * out[i];
        }
    }
    out[chart] = (1.0 - sum).max(0.0).sqrt();
    out
}

/// The BCC-lattice Gibbs variant at 2+1+29, for comparison only.
///
/// Slightly better error than the shipped codec, at the price of two integer
/// divisions and two modulos by `N = 812` in the decode — on top of the same
/// normalize. That is the trade the crate declines.
fn gibbs_bcc_linear(q: [f64; 4]) -> [f64; 4] {
    const N: i64 = 812;
    let mut chart = 0;
    for i in 1..4 {
        if q[i].abs() > q[chart].abs() {
            chart = i;
        }
    }
    let signed = if q[chart] < 0.0 {
        [-q[0], -q[1], -q[2], -q[3]]
    } else {
        q
    };
    let pivot = signed[chart];

    // The Gibbs vector, in the cube [-1, 1]^3.
    let mut t = [0.0f64; 3];
    let mut slot = 0;
    for i in 0..4 {
        if i != chart {
            t[slot] = signed[i] / pivot;
            slot += 1;
        }
    }

    // Quantize onto the body-centred cubic lattice: the integer grid plus the
    // same grid offset by half a cell, whichever is nearer.
    let scale = f64::from(N as u32) / 2.0;
    let mut best = [0.0f64; 3];
    let mut best_error = f64::INFINITY;
    for offset in [0.0, 0.5] {
        let mut candidate = [0.0f64; 3];
        let mut error = 0.0;
        for k in 0..3 {
            let cell = ((t[k] * scale) - offset).round() + offset;
            let clamped = cell.clamp(-scale, scale);
            candidate[k] = clamped / scale;
            error += (candidate[k] - t[k]) * (candidate[k] - t[k]);
        }
        if error < best_error {
            best_error = error;
            best = candidate;
        }
    }

    let mut out = [0.0f64; 4];
    out[chart] = 1.0;
    let mut slot = 0;
    for i in 0..4 {
        if i != chart {
            out[i] = best[slot];
            slot += 1;
        }
    }
    let norm = out.iter().map(|c| c * c).sum::<f64>().sqrt();
    [out[0] / norm, out[1] / norm, out[2] / norm, out[3] / norm]
}

/// Measures one codec and prints its row.
fn measure(name: &str, bits: u32, mut codec: impl FnMut([f64; 4]) -> [f64; 4]) {
    let mut rng = Rng(0x2024_c0de_face_b00c);
    let mut worst = 0.0f64;
    let mut total = 0.0f64;

    for _ in 0..SAMPLES {
        let reference = random_quaternion(&mut rng);
        let error = angle_degrees(reference, codec(reference));
        worst = worst.max(error);
        total += error;
    }

    println!(
        "  {name:<34} {bits:>3} b  {:>8.4}  {:>8.4}",
        total / f64::from(SAMPLES),
        worst
    );
}

/// Samples for the endpoint table, which counts rare events and needs more of
/// them than the codec table does.
const ENDPOINT_SAMPLES: u32 = 1_000_000;

/// Measures what an interpolation at an endpoint computes when it is not
/// short-circuited, and prints the table the READMEs quote.
///
/// Nothing is reverted to get these. Each of the four operations reduces to a
/// routine that is still public, so the drift the short-circuits hide can be
/// measured by calling that routine directly:
///
/// * `Versor::nlerp` at `Factor32::ZERO` mixes `self` with `to` in proportion
///   zero — which is `self` — and ends on `normalize4`. `renormalize` *is*
///   `normalize4` and nothing else, so `a.renormalize()` is bit for bit what
///   the unguarded `nlerp` returned.
/// * `Basis::nlerp` at `ZERO` adds a conversion at each end of that.
/// * `Transform::lerp` and `GlobalFineTransform::lerp` at either endpoint leave the
///   versor alone — `Versor::nlerp` has its own guard — and then repack it, so
///   the drift is the codec's round trip and nothing else.
fn measure_endpoints() {
    let mut rng = Rng(0x2024_e4d9_0147_5f01);
    let mut counts = [0u32; 4];
    let mut worst = [0.0f64; 4];
    // The `acos` form on the fine codec's pairs, for the comparison the
    // `angle_to` documentation makes.
    let mut worst_acos = 0.0f64;

    for _ in 0..ENDPOINT_SAMPLES {
        let a = to_versor(random_quaternion(&mut rng));

        let renormalized = a.renormalize();
        if renormalized != a {
            counts[0] += 1;
            worst[0] = worst[0].max(angle_degrees(from_versor(a), from_versor(renormalized)));
        }

        let m = a.to_basis();
        let round_tripped = Versor::from_basis(m).renormalize().to_basis();
        if round_tripped != m {
            counts[1] += 1;
            let (p, q) = (Versor::from_basis(m), Versor::from_basis(round_tripped));
            worst[1] = worst[1].max(angle_degrees(from_versor(p), from_versor(q)));
        }

        let coarse = Rotation::from_versor(a);
        let coarse_again = Rotation::from_versor(coarse.to_versor());
        if coarse_again != coarse {
            counts[2] += 1;
            worst[2] = worst[2].max(angle_degrees(
                from_versor(coarse.to_versor()),
                from_versor(coarse_again.to_versor()),
            ));
        }

        let fine = FineRotation::from_versor(a);
        let fine_again = FineRotation::from_versor(fine.to_versor());
        if fine_again != fine {
            counts[3] += 1;
            worst[3] = worst[3].max(angle_degrees(
                from_versor(fine.to_versor()),
                from_versor(fine_again.to_versor()),
            ));
            worst_acos = worst_acos.max(
                fine.to_versor()
                    .angle_to(fine_again.to_versor())
                    .to_degrees(),
            );
        }
    }

    println!("\nendpoint drift the short-circuits hide ({ENDPOINT_SAMPLES} uniform samples)");
    println!(
        "  {:<40} {:>10}  {:>10}",
        "operation at an endpoint", "moved", "max"
    );
    for (name, i) in [
        ("Versor::nlerp / slerp", 0),
        ("Basis::nlerp / slerp", 1),
        ("Transform::lerp (Rotation repack)", 2),
        ("GlobalFineTransform::lerp (FineRotation repack)", 3),
    ] {
        println!("  {name:<40} {:>10}  {:>11.9}°", counts[i], worst[i]);
    }
    println!();
    println!("  The coarse repack's max is a flat zero: the pairs it moves are two codes");
    println!("  for one rotation, so the bits change and the versor does not.");
    println!("  Versor::angle_to reads the fine codec's worst pair as {worst_acos:.4}°, against");
    println!(
        "  {:.9}° by the chord form — `acos` is ill-conditioned at 1.\n",
        worst[3]
    );
}

fn main() {
    println!("\nrotation codec quality ({SAMPLES} uniform samples, f64 reference, chord metric)");
    println!(
        "  {:<34} {:>5}  {:>8}  {:>8}",
        "codec", "size", "mean", "max"
    );

    measure("gibbs linear 2+10+10+10 (shipped)", 32, gibbs_linear);
    measure("smallest-three 2+10+10+10", 32, smallest_three);
    measure("gibbs bcc linear 2+1+29", 32, gibbs_bcc_linear);
    measure("4x Signed16 quaternion (shipped)", 64, fine_quaternion);

    println!(
        "\n  budgets: 1/5 deg = 0.2000 for the 32-bit tier, 1/128 deg = 0.0078 for the 64-bit tier"
    );

    measure_endpoints();
}
