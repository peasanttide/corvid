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
use corvid_fixed::{Factor32, I16F16, I24F8, I48F16, Signed32};
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

// --- normalize -------------------------------------------------------------

#[test]
fn normalize_returns_none_only_at_zero() {
    assert_eq!(GlobalPoint::ZERO.normalize(), None);
    assert_eq!(GlobalFinePoint::ZERO.normalize(), None);
    assert_eq!(FinePoint::ZERO.normalize(), None);
    assert_eq!(Direction::ZERO.normalize(), None);

    // A single last-bit component still has a direction.
    let tiny = GlobalPoint::new(I24F8::DELTA, I24F8::ZERO, I24F8::ZERO);
    assert_eq!(tiny.normalize().map(Direction::x), Some(Signed32::MAX));

    let tiny_negative = GlobalPoint::new(I24F8::ZERO, I24F8::ZERO, -I24F8::DELTA);
    assert_eq!(
        tiny_negative.normalize().map(Direction::z),
        Some(Signed32::MIN)
    );
}

#[test]
fn normalize_matches_an_f64_reference() {
    let mut rng = Rng::new(0x0_0F1E);
    let mut worst = 0.0f64;
    for _ in 0..50_000 {
        let p = common::random_global_point(&mut rng, 10_000.0);
        let Some(unit) = p.normalize() else { continue };

        let length =
            (p.x().to_f64().powi(2) + p.y().to_f64().powi(2) + p.z().to_f64().powi(2)).sqrt();
        for (actual, raw) in [
            (unit.x().to_f64(), p.x().to_f64()),
            (unit.y().to_f64(), p.y().to_f64()),
            (unit.z().to_f64(), p.z().to_f64()),
        ] {
            worst = worst.max((actual - raw / length).abs());
        }
    }
    // A unit vector's own last bit is 4.7e-10; the shift-based rescale costs a
    // few more of them on the smallest component.
    assert!(worst < 1e-7, "worst component error {worst}");
}

#[test]
fn a_normalized_vector_has_unit_length() {
    let mut rng = Rng::new(0x0117);
    for _ in 0..50_000 {
        let p = common::random_global_fine_point(&mut rng, 1.0e13);
        let Some(unit) = p.normalize() else { continue };
        let length_squared = unit.length_squared() as f64 / (2_147_483_647.0 * 2_147_483_647.0);
        assert!(
            (length_squared - 1.0).abs() < 1e-6,
            "{unit:?} has squared length {length_squared}"
        );
    }
}

#[test]
fn normalize_depends_only_on_the_ratios_of_the_components() {
    // Scaling the input by 100 does not change the direction -- to within the
    // last bit or two, which is all the shift-based rescale promises. It is
    // deterministic, not magnitude-independent to the bit.
    let coarse = GlobalPoint::new(
        I24F8::from_f64(3.0),
        I24F8::from_f64(4.0),
        I24F8::from_f64(12.0),
    );
    let scaled = GlobalPoint::new(
        I24F8::from_f64(300.0),
        I24F8::from_f64(400.0),
        I24F8::from_f64(1200.0),
    );
    let a = coarse.normalize().expect("non-zero");
    let b = scaled.normalize().expect("non-zero");
    for (p, q) in a.to_array().iter().zip(b.to_array().iter()) {
        assert!((p.to_f64() - q.to_f64()).abs() < 1e-8, "{a:?} vs {b:?}");
    }

    let unit = coarse.normalize().expect("non-zero");
    assert!((unit.x().to_f64() - 3.0 / 13.0).abs() < 1e-7);
    assert!((unit.y().to_f64() - 4.0 / 13.0).abs() < 1e-7);
    assert!((unit.z().to_f64() - 12.0 / 13.0).abs() < 1e-7);
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

// --- normalize_fast --------------------------------------------------------
//
// The approximate tier answers to a bound rather than to a bit pattern, so
// these mirror the exact tier's tests with the bound substituted for exactness.

/// What a `3.2e-5` relative `rsqrt` costs a unit component, with room for the
/// rounding the rescale in step 4 adds on top.
const NORMALIZE_FAST_TOLERANCE: f64 = 4e-5;

#[test]
fn normalize_fast_returns_none_exactly_where_normalize_does() {
    assert_eq!(GlobalPoint::ZERO.normalize_fast(), None);
    assert_eq!(GlobalFinePoint::ZERO.normalize_fast(), None);
    assert_eq!(FinePoint::ZERO.normalize_fast(), None);
    assert_eq!(Direction::ZERO.normalize_fast(), None);

    // The axis-aligned cases are taken by hand before the `rsqrt`, so they stay
    // exactly `+/-1` in this tier too.
    let tiny = GlobalPoint::new(I24F8::DELTA, I24F8::ZERO, I24F8::ZERO);
    assert_eq!(tiny.normalize_fast().map(Direction::x), Some(Signed32::MAX));

    let tiny_negative = GlobalPoint::new(I24F8::ZERO, I24F8::ZERO, -I24F8::DELTA);
    assert_eq!(
        tiny_negative.normalize_fast().map(Direction::z),
        Some(Signed32::MIN)
    );
}

#[test]
fn normalize_fast_matches_an_f64_reference_to_its_bound() {
    let mut rng = Rng::new(0x0_0F1E);
    let mut worst = 0.0f64;
    for _ in 0..50_000 {
        let p = common::random_global_point(&mut rng, 10_000.0);
        let Some(unit) = p.normalize_fast() else {
            continue;
        };

        let length =
            (p.x().to_f64().powi(2) + p.y().to_f64().powi(2) + p.z().to_f64().powi(2)).sqrt();
        for (actual, raw) in [
            (unit.x().to_f64(), p.x().to_f64()),
            (unit.y().to_f64(), p.y().to_f64()),
            (unit.z().to_f64(), p.z().to_f64()),
        ] {
            worst = worst.max((actual - raw / length).abs());
        }
    }
    assert!(
        worst < NORMALIZE_FAST_TOLERANCE,
        "worst component error {worst}"
    );
    // And it is genuinely the coarser tier: the exact one holds this same sweep
    // under 1e-7, so a `worst` that small would mean the two had converged.
    assert!(worst > 1e-7, "normalize_fast is no longer approximate");
}

#[test]
fn a_fast_normalized_vector_still_has_unit_length() {
    let mut rng = Rng::new(0x0117);
    for _ in 0..50_000 {
        let p = common::random_global_fine_point(&mut rng, 1.0e13);
        let Some(unit) = p.normalize_fast() else {
            continue;
        };
        let length_squared = unit.length_squared() as f64 / (2_147_483_647.0 * 2_147_483_647.0);
        // Squaring doubles the relative error, which is the whole of the
        // difference between this bound and the exact tier's 1e-6.
        assert!(
            (length_squared - 1.0).abs() < 1e-4,
            "{unit:?} has squared length {length_squared}"
        );
    }
}

#[test]
fn direction_to_fast_agrees_with_direction_to_to_its_bound() {
    let mut rng = Rng::new(0x0D1E_C710);
    for _ in 0..20_000 {
        let a = common::random_global_point(&mut rng, 10_000.0);
        let b = common::random_global_point(&mut rng, 10_000.0);
        let (Some(exact), Some(fast)) = (a.direction_to(b), a.direction_to_fast(b)) else {
            continue;
        };
        for (x, y) in [
            (exact.x().to_f64(), fast.x().to_f64()),
            (exact.y().to_f64(), fast.y().to_f64()),
            (exact.z().to_f64(), fast.z().to_f64()),
        ] {
            assert!(
                (x - y).abs() < NORMALIZE_FAST_TOLERANCE,
                "direction_to gave {x}, direction_to_fast gave {y}"
            );
        }
    }
}

#[test]
fn the_signed32_denormal_computes_as_the_value_it_denotes() {
    // `SNORM` spends `i32::MIN` and `-(2^31 - 1)` on the same `-1.0`, and
    // `corvid_fixed` resolves that by comparing, hashing and calculating on the
    // canonical form. Everything here reads bit patterns directly, so it has to
    // fold the denormal too -- otherwise two `Direction`s that compare equal and
    // hash alike come back with different lengths and different normalized
    // directions, which is precisely the `Hash`/`Eq` disagreement the
    // convention exists to prevent.
    let denormal = Signed32::from_bits(i32::MIN);
    let unit_y = Direction::new(Signed32::ZERO, Signed32::MAX, Signed32::ZERO);
    assert_eq!(denormal, Signed32::MIN, "the scalars themselves agree");
    assert!(denormal.is_denormal());

    // Every component position, and a mixture that exercises the normalize's
    // largest-component rescale from both sides.
    for (a, b) in [
        (
            Direction::new(denormal, Signed32::ZERO, Signed32::ZERO),
            Direction::new(Signed32::MIN, Signed32::ZERO, Signed32::ZERO),
        ),
        (
            Direction::new(Signed32::MAX, denormal, Signed32::ZERO),
            Direction::new(Signed32::MAX, Signed32::MIN, Signed32::ZERO),
        ),
        (
            Direction::new(Signed32::from_bits(7), Signed32::MAX, denormal),
            Direction::new(Signed32::from_bits(7), Signed32::MAX, Signed32::MIN),
        ),
        (
            Direction::new(denormal, denormal, denormal),
            Direction::new(Signed32::MIN, Signed32::MIN, Signed32::MIN),
        ),
    ] {
        assert_eq!(a, b, "the directions compare equal");
        assert_eq!(a.length_squared(), b.length_squared(), "length_squared");
        assert_eq!(a.length().to_bits(), b.length().to_bits(), "length");
        assert_eq!(a.dot(a), b.dot(b), "dot");
        assert_eq!(
            a.cross(unit_y).to_array(),
            b.cross(unit_y).to_array(),
            "cross"
        );
        assert_eq!(a.to_fine().to_array(), b.to_fine().to_array(), "to_fine");
        assert_eq!(
            a.to_global().to_array(),
            b.to_global().to_array(),
            "to_global"
        );
        assert_eq!(
            a.to_global_fine().to_array(),
            b.to_global_fine().to_array(),
            "to_global_fine"
        );
        assert_eq!(
            a.normalize().map(Direction::to_array),
            b.normalize().map(Direction::to_array),
            "normalize"
        );
        assert_eq!(
            a.normalize_fast().map(Direction::to_array),
            b.normalize_fast().map(Direction::to_array),
            "normalize_fast"
        );
        assert_eq!(
            a.direction_to(unit_y).map(Direction::to_array),
            b.direction_to(unit_y).map(Direction::to_array),
            "direction_to"
        );
    }
}
