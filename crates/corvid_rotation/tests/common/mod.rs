//! Helpers shared by the test binaries.

#![allow(
    clippy::missing_const_for_fn,
    reason = "the helpers are only ever called at run time"
)]
#![allow(
    unreachable_pub,
    dead_code,
    reason = "each test binary includes this module and uses a different subset of it"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    reason = "test helpers convert between widths freely; the values are bounded by construction"
)]

/// A deterministic xorshift64\* generator.
///
/// Used where a domain is too large to walk exhaustively. Deterministic on
/// purpose: a failure reported by CI reproduces exactly on a developer machine.
pub struct Rng(u64);

impl Rng {
    /// Seeds the generator. Any non-zero seed works.
    pub const fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    /// The next 64 bits.
    pub const fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// The next 32 bits.
    pub const fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// A value in `-1.0 ..= 1.0`.
    pub fn next_unit(&mut self) -> f64 {
        f64::from(self.next_u32()).mul_add(2.0 / f64::from(u32::MAX), -1.0)
    }
}

/// Every `i32` worth reaching for when probing boundaries.
pub const I32_EDGES: &[i32] = &[
    i32::MIN,
    i32::MIN + 1,
    -65_537,
    -65_536,
    -257,
    -256,
    -255,
    -2,
    -1,
    0,
    1,
    2,
    255,
    256,
    257,
    65_536,
    65_537,
    i32::MAX - 1,
    i32::MAX,
];

/// Reports the worst mismatch found while sweeping a domain.
#[derive(Default)]
pub struct Worst {
    /// Largest absolute error seen, in units of the output's last bit.
    pub error: i128,
    /// The input that produced it.
    pub at: i128,
    /// How many inputs were off by anything at all.
    pub inexact: u64,
    /// How many inputs were checked.
    pub checked: u64,
}

impl Worst {
    /// Folds one comparison in.
    pub const fn observe(&mut self, input: i128, actual: i128, expected: i128) {
        self.checked += 1;
        let error = (actual - expected).abs();
        if error > 0 {
            self.inexact += 1;
        }
        if error > self.error {
            self.error = error;
            self.at = input;
        }
    }

    /// Asserts the worst error stayed within `limit` last-bit units.
    pub fn assert_within(&self, limit: i128, what: &str) {
        assert!(
            self.error <= limit,
            "{what}: worst error {} bits (limit {limit}) at input {}; {} of {} inputs inexact",
            self.error,
            self.at,
            self.inexact,
            self.checked
        );
    }
}

// --- rotation helpers ------------------------------------------------------

use corvid_rotation::{Basis, Versor};
use corvid_vector::{Direction, FinePoint};

/// A uniformly distributed unit quaternion, in `f64`, by Shoemake's method.
///
/// The reference the codecs are measured against, so it stays in `f64`
/// throughout: an `f32` reference has a noise floor near 0.05 degrees, which at
/// `FineRotation`'s 0.0034 degrees would be measuring the harness rather than
/// the codec.
pub fn random_unit_quaternion_f64(rng: &mut Rng) -> [f64; 4] {
    let u1 = f64::from(rng.next_u32()) / f64::from(u32::MAX);
    let u2 = f64::from(rng.next_u32()) / f64::from(u32::MAX);
    let u3 = f64::from(rng.next_u32()) / f64::from(u32::MAX);
    let (r1, r2) = ((1.0 - u1).sqrt(), u1.sqrt());
    let (t1, t2) = (core::f64::consts::TAU * u2, core::f64::consts::TAU * u3);
    // (x, y, z, w)
    [r1 * t1.sin(), r1 * t1.cos(), r2 * t2.sin(), r2 * t2.cos()]
}

/// The versor nearest an `f64` quaternion.
pub fn versor_from_f64(q: [f64; 4]) -> Versor {
    let bits = |v: f64| corvid_fixed::I2F30::from_f64(v);
    // The fallback is only reachable if the input was not unit, which the
    // generator above never produces.
    Versor::from_xyzw(bits(q[0]), bits(q[1]), bits(q[2]), bits(q[3])).unwrap_or(Versor::IDENTITY)
}

/// A versor's components as `f64`.
pub const fn to_f64_quaternion(q: Versor) -> [f64; 4] {
    let [x, y, z, w] = q.to_xyzw();
    [x.to_f64(), y.to_f64(), z.to_f64(), w.to_f64()]
}

/// A uniformly distributed rotation.
pub fn random_versor(rng: &mut Rng) -> Versor {
    versor_from_f64(random_unit_quaternion_f64(rng))
}

/// A uniformly distributed rotation, as a matrix.
pub fn random_basis(rng: &mut Rng) -> Basis {
    random_versor(rng).to_basis()
}

/// The angle between two quaternions in degrees, by the **chord form**
/// `4 · asin(chord / 2)`.
///
/// Never `2 · acos(|q1 · q2|)`: that form has an `f32` noise floor near 0.05
/// degrees and loses precision near zero even in `f64`, which is the
/// measurement pitfall the source paper documents.
pub fn angle_degrees(a: [f64; 4], b: [f64; 4]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(p, q)| p * q).sum();
    // Take the nearer member of the double-cover pair.
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
    4.0 * (chord / 2.0).clamp(-1.0, 1.0).asin() * 180.0 / core::f64::consts::PI
}

/// A `FinePoint` with each component uniform in `-range ..= range`.
pub fn random_fine_point(rng: &mut Rng, range: f64) -> FinePoint {
    FinePoint::new(
        corvid_fixed::I16F16::from_f64(rng.next_unit() * range),
        corvid_fixed::I16F16::from_f64(rng.next_unit() * range),
        corvid_fixed::I16F16::from_f64(rng.next_unit() * range),
    )
}

/// A uniformly distributed unit direction.
pub fn random_direction(rng: &mut Rng) -> Direction {
    loop {
        let candidate = corvid_vector::GlobalPoint::new(
            corvid_fixed::I24F8::from_f64(rng.next_unit()),
            corvid_fixed::I24F8::from_f64(rng.next_unit()),
            corvid_fixed::I24F8::from_f64(rng.next_unit()),
        );
        if let Some(direction) = candidate.normalize() {
            return direction;
        }
    }
}

/// The negation of a direction.
pub fn neg(d: Direction) -> Direction {
    -d
}

/// Returns `true` if two directions agree to within `tolerance` per component.
pub fn direction_within(a: Direction, b: Direction, tolerance: f64) -> bool {
    (a.x().to_f64() - b.x().to_f64()).abs() <= tolerance
        && (a.y().to_f64() - b.y().to_f64()).abs() <= tolerance
        && (a.z().to_f64() - b.z().to_f64()).abs() <= tolerance
}

/// Returns `true` if two near-field points agree to within `tolerance`.
pub fn within(a: FinePoint, b: FinePoint, tolerance: corvid_fixed::I16F16) -> bool {
    let limit = i64::from(tolerance.to_bits());
    let close = |p: corvid_fixed::I16F16, q: corvid_fixed::I16F16| {
        (i64::from(p.to_bits()) - i64::from(q.to_bits())).abs() <= limit
    };
    close(a.x(), b.x()) && close(a.y(), b.y()) && close(a.z(), b.z())
}
