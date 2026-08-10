//! Normalizing, in both tiers, against an `f64` reference.
//!
//! Turning a vector into a unit direction is the one operation here that is not
//! a component at a time: the scale cancels, so what is checked is that the
//! answer depends on the ratios and nothing else, that it comes out a unit
//! vector, and that the fast tier stays inside the bound the crate claims for
//! it. `tests/vector.rs` holds the arithmetic and the geometry.

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

use common::Rng;
use corvid_fixed::{I24F8, Signed32};
use corvid_vector::{Direction, FinePoint, GlobalFinePoint, GlobalPoint};

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

/// A ratio names a direction at any scale, and zero names none.
///
/// The scale cancels, which is the property that makes this worth having: a
/// caller whose vector is a cross product of far-apart points has a number
/// that fits no component, and narrowing it to fit would answer a different
/// direction rather than a rounder one.
#[test]
fn a_wide_ratio_normalizes_at_any_scale() {
    assert_eq!(Direction::from_ratio([0, 1, 0]), Some(Direction::Y));
    assert_eq!(Direction::from_ratio([0, 5, 0]), Some(Direction::Y));
    assert_eq!(Direction::from_ratio([-1, 0, 0]), Some(-Direction::X));

    // Far past what a component holds, and up to the widest ratio there is.
    assert_eq!(Direction::from_ratio([0, 0, i64::MAX]), Some(Direction::Z));
    assert_eq!(Direction::from_ratio([0, 0, i64::MIN]), Some(-Direction::Z));

    assert_eq!(Direction::from_ratio([0, 0, 0]), None);
}

/// Doubling a ratio does not move the direction at all.
///
/// The reduction inside shifts by a power of two to bring the largest
/// component just under `2^30`, so a scale that is itself a power of two
/// changes the shift and nothing else, and the answer is bit-identical. That
/// is the invariance worth pinning: two peers that scaled their arithmetic by
/// different powers of two agree exactly.
#[test]
fn scaling_a_ratio_by_a_power_of_two_does_not_move_the_direction() {
    for shift in 0..40 {
        let scale = 1_i64 << shift;
        assert_eq!(
            Direction::from_ratio([3 * scale, -4 * scale, 12 * scale]),
            Direction::from_ratio([3, -4, 12]),
            "a ratio scaled by 2^{shift} named a different direction",
        );
    }
}

/// Any other scale agrees to within a last bit or so, and not exactly.
///
/// Worth stating as a test rather than left as a surprise: the shift above is
/// by a whole number of bits, so a scale of seven lands the mantissa
/// differently and the last places diverge. A caller comparing two directions
/// that reached the same ratio by different arithmetic has to expect that.
#[test]
fn any_other_scale_agrees_to_within_a_last_bit() {
    let unscaled = Direction::from_ratio([3, -4, 12]).expect("non-zero");
    for scale in [7_i64, 1_000, 1_000_000_007] {
        let scaled = Direction::from_ratio([3 * scale, -4 * scale, 12 * scale]).expect("non-zero");
        for axis in 0..3 {
            let apart =
                (scaled.to_array()[axis].to_f64() - unscaled.to_array()[axis].to_f64()).abs();
            assert!(
                apart < 1e-8,
                "a ratio scaled by {scale} moved axis {axis} by {apart}",
            );
        }
    }
}
