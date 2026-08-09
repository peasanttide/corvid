//! Construction, arithmetic and geometry, against an `f64` reference.

#![allow(
    clippy::float_cmp,
    reason = "comparisons are against exactly representable references"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    reason = "these tests feed edge-case bit patterns through narrowing casts on purpose"
)]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::suboptimal_flops,
    reason = "the f64 references are written as plain sums on purpose; mul_add would fuse the rounding and stop being an independent reference"
)]

mod common;

use std::hint::black_box;

use common::Rng;
use corvid_fixed::{Factor32, I16F16, I24F8, I48F16};
use corvid_vector::{Direction, FinePoint, GlobalFinePoint, GlobalPoint};

#[test]
fn construction_and_accessors_round_trip() {
    let p = GlobalPoint::new(
        I24F8::from_f64(1.0),
        I24F8::from_f64(-2.0),
        I24F8::from_f64(3.5),
    );
    assert_eq!(p.x().to_f64(), 1.0);
    assert_eq!(p.y().to_f64(), -2.0);
    assert_eq!(p.z().to_f64(), 3.5);
    assert_eq!(GlobalPoint::from_array(p.to_array()), p);
    assert_eq!(GlobalPoint::splat(I24F8::ZERO), GlobalPoint::ZERO);
    assert_eq!(GlobalPoint::default(), GlobalPoint::ZERO);
    assert!(GlobalPoint::ZERO.is_zero());
    assert!(!p.is_zero());
}

#[test]
fn arithmetic_matches_an_f64_reference() {
    let mut rng = Rng::new(0xA817_4437);
    for _ in 0..50_000 {
        let a = common::random_global_point(&mut rng, 1_000.0);
        let b = common::random_global_point(&mut rng, 1_000.0);

        let sum = a + b;
        assert_eq!(sum.x().to_f64(), a.x().to_f64() + b.x().to_f64());
        assert_eq!(sum.y().to_f64(), a.y().to_f64() + b.y().to_f64());

        let difference = a - b;
        assert_eq!(difference.z().to_f64(), a.z().to_f64() - b.z().to_f64());

        assert_eq!((-a).x().to_f64(), -a.x().to_f64());
        assert_eq!(a.abs().x().to_f64(), a.x().to_f64().abs());
        assert_eq!(a.min(b).y(), a.y().min(b.y()));
        assert_eq!(a.max(b).y(), a.y().max(b.y()));
    }
}

#[test]
fn addition_saturates_rather_than_wrapping() {
    let big = GlobalPoint::splat(I24F8::MAX);
    assert_eq!(big + big, big);
    assert_eq!(big.checked_add(big), None);

    let wide = GlobalFinePoint::splat(I48F16::MAX);
    assert_eq!(wide + wide, wide);
    assert_eq!(wide.checked_add(wide), None);

    // Component-wise: only the axis that overflows clamps.
    let mixed = GlobalPoint::new(I24F8::MAX, I24F8::ZERO, I24F8::ZERO);
    let one = GlobalPoint::splat(I24F8::ONE);
    assert_eq!((mixed + one).x(), I24F8::MAX);
    assert_eq!((mixed + one).y(), I24F8::ONE);
    assert_eq!(mixed.checked_add(one), None);
}

#[test]
fn scaling_by_a_scalar_is_component_wise() {
    let p = GlobalPoint::new(
        I24F8::from_f64(1.0),
        I24F8::from_f64(-2.0),
        I24F8::from_f64(3.0),
    );
    let doubled = p * I24F8::from_f64(2.0);
    assert_eq!(doubled.y().to_f64(), -4.0);
    assert_eq!(p.checked_mul(I24F8::MAX), None);
}

#[test]
fn lerp_is_exact_at_both_ends() {
    let a = GlobalPoint::splat(I24F8::from_f64(-1000.0));
    let b = GlobalPoint::splat(I24F8::from_f64(1000.0));
    assert_eq!(a.lerp(b, Factor32::ZERO), a);
    assert_eq!(a.lerp(b, Factor32::ONE), b);
    assert_eq!(a.lerp(b, Factor32::from_f64(0.5)), GlobalPoint::ZERO);
}

#[test]
fn clamp_is_component_wise_and_cannot_panic() {
    let low = GlobalPoint::splat(I24F8::from_f64(-1.0));
    let high = GlobalPoint::splat(I24F8::from_f64(1.0));
    let outside = GlobalPoint::new(
        I24F8::from_f64(-5.0),
        I24F8::from_f64(0.5),
        I24F8::from_f64(5.0),
    );
    let clamped = outside.clamp(low, high);
    assert_eq!(clamped.x().to_f64(), -1.0);
    assert_eq!(clamped.y().to_f64(), 0.5);
    assert_eq!(clamped.z().to_f64(), 1.0);

    // Inverted bounds do not panic; the bound applied last wins.
    let _ = outside.clamp(high, low);
}

// --- geometry --------------------------------------------------------------

#[test]
fn length_squared_is_lossless_where_the_component_type_would_saturate() {
    // 2 km on each axis. The sum of squares in DELTA^2 units is 7.9e17, which
    // I24F8 cannot hold -- the u64 return is exact anyway.
    let p = GlobalPoint::splat(I24F8::from_f64(2000.0));
    let component = i64::from(I24F8::from_f64(2000.0).to_bits());
    assert_eq!(p.length_squared(), 3 * (component * component) as u64);

    // And the same vector's length lands where f64 says it does.
    let expected = 3.0f64.sqrt() * 2000.0;
    assert!((p.length().to_f64() - expected).abs() < 0.01);
}

#[test]
fn length_squared_at_the_corner_does_not_overflow() {
    let corner = GlobalPoint::splat(I24F8::MAX);
    let c = i64::from(I24F8::MAX.to_bits()) as u64;
    assert_eq!(corner.length_squared(), 3 * c * c);

    let far = GlobalFinePoint::splat(I48F16::MAX);
    let f = I48F16::MAX.to_bits() as u128;
    assert_eq!(far.length_squared(), 3 * f * f);
}

#[test]
fn length_matches_an_f64_reference() {
    let mut rng = Rng::new(0x1E_9714);
    for _ in 0..50_000 {
        let p = common::random_global_point(&mut rng, 100_000.0);
        let reference =
            (p.x().to_f64().powi(2) + p.y().to_f64().powi(2) + p.z().to_f64().powi(2)).sqrt();
        // One rounding, at the component's own last bit.
        assert!(
            (p.length().to_f64() - reference).abs() <= I24F8::DELTA.to_f64(),
            "{p:?}: got {}, want {reference}",
            p.length().to_f64()
        );
    }
}

#[test]
fn distance_saturates_only_where_the_type_genuinely_runs_out() {
    // sqrt(3) * 1.407e14 exceeds I48F16's own range, so the diagonal of the
    // whole world saturates. Documented, and this is where it is documented.
    let lo = GlobalFinePoint::splat(I48F16::MIN);
    let hi = GlobalFinePoint::splat(I48F16::MAX);
    assert_eq!(lo.distance(hi), I48F16::MAX);

    // Anything short of that is exact.
    let a = GlobalFinePoint::ZERO;
    let b = GlobalFinePoint::new(I48F16::from_f64(3.0), I48F16::from_f64(4.0), I48F16::ZERO);
    assert_eq!(a.distance(b), I48F16::from_f64(5.0));
    assert_eq!(b.distance(a), I48F16::from_f64(5.0));
    assert_eq!(a.distance(a), I48F16::ZERO);
}

#[test]
fn cross_follows_the_right_handed_convention() {
    // X x Y = Z, which is what makes right = forward x up consistent.
    let x = GlobalPoint::new(I24F8::ONE, I24F8::ZERO, I24F8::ZERO);
    let y = GlobalPoint::new(I24F8::ZERO, I24F8::ONE, I24F8::ZERO);
    let z = GlobalPoint::new(I24F8::ZERO, I24F8::ZERO, I24F8::ONE);
    assert_eq!(x.cross(y), z);
    assert_eq!(y.cross(z), x);
    assert_eq!(z.cross(x), y);

    // Anti-commutative, and a vector crossed with itself is zero.
    assert_eq!(y.cross(x), -z);
    assert_eq!(x.cross(x), GlobalPoint::ZERO);
}

#[test]
fn cross_is_perpendicular_to_both_operands() {
    let mut rng = Rng::new(0xC1055);
    for _ in 0..20_000 {
        let a = common::random_global_point(&mut rng, 100.0);
        let b = common::random_global_point(&mut rng, 100.0);
        let c = a.cross(b);

        // The dot products are zero up to the cross product's own rounding:
        // each component was rounded to 1/256, and the dot then scales that by
        // the operands' magnitudes.
        let scale = 100.0 * 256.0 * 256.0;
        assert!(
            (a.dot(c) as f64).abs() < scale * 4.0,
            "a . (a x b) = {} for {a:?} x {b:?}",
            a.dot(c)
        );
        assert!((b.dot(c) as f64).abs() < scale * 4.0);
    }
}

#[test]
fn dot_matches_an_f64_reference() {
    let mut rng = Rng::new(0xD07_D07);
    for _ in 0..50_000 {
        let a = common::random_global_point(&mut rng, 1_000.0);
        let b = common::random_global_point(&mut rng, 1_000.0);
        let reference = a.x().to_f64() * b.x().to_f64()
            + a.y().to_f64() * b.y().to_f64()
            + a.z().to_f64() * b.z().to_f64();
        // The result is in units of DELTA^2, and is exact.
        assert_eq!(a.dot(b) as f64 / 65_536.0, reference);
    }
}

// --- const -----------------------------------------------------------------

#[test]
fn every_operation_is_available_in_const_context() {
    const P: GlobalPoint = GlobalPoint::new(
        I24F8::from_f64(3.0),
        I24F8::from_f64(4.0),
        I24F8::from_f64(12.0),
    );
    const LENGTH: I24F8 = P.length();
    const SQUARED: u64 = P.length_squared();
    const UNIT: Option<Direction> = P.normalize();
    const CROSS: GlobalPoint = P.cross(GlobalPoint::splat(I24F8::ONE));
    const NEAR: FinePoint = FinePoint::ZERO;
    const WIDE: GlobalFinePoint = GlobalFinePoint::ZERO;

    assert_eq!(LENGTH, I24F8::from_f64(13.0));
    assert_eq!(SQUARED, black_box(P).length_squared());
    assert_eq!(UNIT, black_box(P).normalize());
    assert_eq!(CROSS, black_box(P).cross(GlobalPoint::splat(I24F8::ONE)));
    assert_eq!(NEAR.length(), I16F16::ZERO);
    assert_eq!(WIDE.length(), I48F16::ZERO);
}
