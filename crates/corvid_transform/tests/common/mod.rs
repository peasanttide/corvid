//! Helpers shared by the test binaries.

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

// --- transform helpers -----------------------------------------------------

use corvid_transform::{
    Angle32, FineRotation, FineTransform, GlobalFinePoint, GlobalPoint, I16F16, I24F8, I48F16,
    Pitch32, Rotation, Transform, Versor,
};

/// A uniformly distributed rotation, as a versor.
pub fn random_versor(rng: &mut Rng) -> Versor {
    let u1 = f64::from(rng.next_u32()) / f64::from(u32::MAX);
    let u2 = f64::from(rng.next_u32()) / f64::from(u32::MAX);
    let u3 = f64::from(rng.next_u32()) / f64::from(u32::MAX);
    let (r1, r2) = ((1.0 - u1).sqrt(), u1.sqrt());
    let (t1, t2) = (core::f64::consts::TAU * u2, core::f64::consts::TAU * u3);
    let c = |v: f64| corvid_transform::I2F30::from_f64(v);
    Versor::from_xyzw(
        c(r1 * t1.sin()),
        c(r1 * t1.cos()),
        c(r2 * t2.sin()),
        c(r2 * t2.cos()),
    )
    .unwrap_or(Versor::IDENTITY)
}

/// A `Transform` with a position uniform in `-range ..= range` per axis.
pub fn random_transform(rng: &mut Rng, range: f64) -> Transform {
    Transform::new(
        GlobalPoint::new(
            I24F8::from_f64(rng.next_unit() * range),
            I24F8::from_f64(rng.next_unit() * range),
            I24F8::from_f64(rng.next_unit() * range),
        ),
        Rotation::from_versor(random_versor(rng)),
    )
}

/// A `FineTransform` with a position uniform in `-range ..= range` per axis.
pub fn random_fine_transform(rng: &mut Rng, range: f64) -> FineTransform {
    FineTransform::new(
        GlobalFinePoint::new(
            I48F16::from_f64(rng.next_unit() * range),
            I48F16::from_f64(rng.next_unit() * range),
            I48F16::from_f64(rng.next_unit() * range),
        ),
        FineRotation::from_versor(random_versor(rng)),
    )
}

/// A point within `radius` metres of `origin`, on each axis.
pub fn near(rng: &mut Rng, origin: GlobalFinePoint, radius: f64) -> GlobalFinePoint {
    origin
        + GlobalFinePoint::new(
            I48F16::from_f64(rng.next_unit() * radius),
            I48F16::from_f64(rng.next_unit() * radius),
            I48F16::from_f64(rng.next_unit() * radius),
        )
}

/// A yaw-pitch-roll pose as a `FineRotation`.
pub fn pose(yaw: f64, pitch: f64, roll: f64) -> FineRotation {
    FineRotation::from_versor(Versor::from_yaw_pitch_roll(
        Angle32::from_degrees(yaw),
        Pitch32::from_degrees(pitch),
        Angle32::from_degrees(roll),
    ))
}

/// Returns `true` if two object-scale points agree to within `tolerance`.
pub fn points_within(a: GlobalPoint, b: GlobalPoint, tolerance: I24F8) -> bool {
    let limit = i64::from(tolerance.to_bits());
    let close =
        |p: I24F8, q: I24F8| (i64::from(p.to_bits()) - i64::from(q.to_bits())).abs() <= limit;
    close(a.x(), b.x()) && close(a.y(), b.y()) && close(a.z(), b.z())
}

/// Returns `true` if two world-scale points agree to within `tolerance`.
pub fn fine_points_within(a: GlobalFinePoint, b: GlobalFinePoint, tolerance: I48F16) -> bool {
    let limit = i128::from(tolerance.to_bits());
    let close =
        |p: I48F16, q: I48F16| (i128::from(p.to_bits()) - i128::from(q.to_bits())).abs() <= limit;
    close(a.x(), b.x()) && close(a.y(), b.y()) && close(a.z(), b.z())
}

/// Returns `true` if a transform is the identity to within the given position
/// and angular tolerances.
pub fn transform_near_identity(t: Transform, position_metres: f64, degrees: f64) -> bool {
    t.position().length().to_f64() <= position_metres
        && t.rotation()
            .to_versor()
            .angle_to(Versor::IDENTITY)
            .to_degrees()
            <= degrees
}

/// Silences the unused-import warning for types only some test binaries use.
#[allow(dead_code, reason = "each test binary uses a different subset")]
pub type UnusedFine = I16F16;
