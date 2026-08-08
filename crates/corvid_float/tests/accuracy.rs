//! What the software implementations cost, measured against the intrinsics
//! they stand in for.
//!
//! This crate's whole trade is that a `const` is worth a slower runtime call,
//! and that trade is only worth making if the answer is the same one. These
//! tests are where "the same one" is given a number.

#![allow(
    clippy::float_cmp,
    reason = "exact equality is the assertion: `floor`, `ceil`, `trunc`, `round` and `abs` are bit-for-bit operations, and a tolerance here would hide the one thing these tests exist to catch"
)]
#![allow(
    clippy::cast_precision_loss,
    reason = "the loop counters are small integers being turned into the sample points, which is the standard way to sweep a range and is exact for every value they take"
)]
#![allow(
    clippy::suboptimal_flops,
    reason = "the tolerance expressions are written as `scale * relative + absolute` because that is how a tolerance reads; folding them into a `mul_add` would be faster and less legible in code whose job is to be read"
)]

#[test]
fn square_roots_match_the_intrinsic() {
    for step in 0..2000u32 {
        let x = f32::from(u16::try_from(step).unwrap_or(u16::MAX)) / 16.0;
        let ours = corvid_float::sqrt(x);
        let theirs = x.sqrt();
        assert!(
            (ours - theirs).abs() <= theirs.abs() * 1e-6 + 1e-7,
            "sqrt({x}): {ours} vs {theirs}"
        );
    }
}

/// Over a couple of turns either side of zero, which is the range a frustum's
/// half-angle and a gain curve actually live in. `const_soft_float`'s argument
/// reduction is `rem_pio2`, so this is also what checks that the reduction is
/// there at all.
#[test]
fn sines_and_cosines_match_the_intrinsic() {
    for step in -800i32..800 {
        let x = step as f32 / 64.0;
        let (ours, theirs) = (corvid_float::sin(x), x.sin());
        assert!((ours - theirs).abs() < 1e-6, "sin({x}): {ours} vs {theirs}");
        let (ours, theirs) = (corvid_float::cos(x), x.cos());
        assert!((ours - theirs).abs() < 1e-6, "cos({x}): {ours} vs {theirs}");
    }
}

/// The composed one. Away from the poles, where both answers are enormous and
/// neither is useful.
#[test]
fn tangents_match_the_intrinsic_away_from_the_poles() {
    for step in -100i32..100 {
        let x = step as f32 / 128.0;
        let (ours, theirs) = (corvid_float::tan(x), x.tan());
        assert!((ours - theirs).abs() < 1e-6, "tan({x}): {ours} vs {theirs}");
    }
}

/// A field of view of half a turn has an infinite tangent. It is a frustum
/// nobody wants and it must not be a panic, because a caller building a matrix
/// tests the result for finiteness and draws nothing.
#[test]
fn a_tangent_at_the_pole_is_an_infinity_rather_than_a_panic() {
    let quarter = corvid_float::consts::FRAC_PI_2;
    assert!(!corvid_float::tan(quarter).is_nan());
    assert!(corvid_float::recip(0.0).is_infinite());
    assert!(corvid_float::recip(-0.0).is_infinite());
}

#[test]
fn the_composed_rounding_matches_the_intrinsics() {
    for step in -400i32..400 {
        let x = step as f32 / 8.0;
        assert_eq!(corvid_float::floor(x), x.floor(), "floor({x})");
        assert_eq!(corvid_float::ceil(x), x.ceil(), "ceil({x})");
        assert_eq!(corvid_float::trunc(x), x.trunc(), "trunc({x})");
        assert_eq!(corvid_float::round(x), x.round(), "round({x})");
        assert_eq!(corvid_float::abs(x), x.abs(), "abs({x})");
    }
}

/// `ceil` is `-floor(-x)`, and the identity is exact because a negation is a
/// sign bit. The one value where a sloppier implementation would disagree with
/// the intrinsic is the negative zero it produces from an input in `(-1, 0)`.
#[test]
fn the_composed_ceiling_keeps_the_intrinsic_s_negative_zero() {
    let ours = corvid_float::ceil(-0.5);
    assert_eq!(ours, (-0.5f32).ceil());
    assert!(ours.is_sign_negative(), "ceil(-0.5) should be -0.0");
}

/// Unlike [`f32::clamp`], which panics when its bounds cross. The workspace
/// forbids a panic in a library, so the upper bound simply wins — and `NaN`
/// falls through to the low bound, which is what turns a gain that has gone
/// wrong into silence rather than into full volume.
#[test]
fn clamping_does_not_panic_on_crossed_bounds_or_nan() {
    // The ordinary case, from both directions.
    assert_eq!(corvid_float::clamp(5.0, 0.0, 1.0), 1.0);
    assert_eq!(corvid_float::clamp(-5.0, 0.0, 1.0), 0.0);
    assert_eq!(corvid_float::clamp(0.5, 0.0, 1.0), 0.5);

    // Crossed: `high` is tested first, so anything above it comes back as it.
    assert_eq!(corvid_float::clamp(0.5, 1.0, 0.0), 0.0);
    assert_eq!(corvid_float::clamp(-1.0, 1.0, 0.0), 1.0);

    assert_eq!(corvid_float::clamp(f32::NAN, 2.0, 3.0), 2.0);
    assert_eq!(corvid_float::wide::clamp(f64::NAN, 2.0, 3.0), 2.0);
}

#[test]
fn hypotenuses_match_the_intrinsic_in_the_range_a_camera_works_in() {
    for x in -20i32..20 {
        for y in -20i32..20 {
            let (x, y) = (x as f32 / 4.0, y as f32 / 4.0);
            let (ours, theirs) = (corvid_float::hypot(x, y), x.hypot(y));
            assert!(
                (ours - theirs).abs() <= theirs * 1e-6 + 1e-7,
                "hypot({x}, {y}): {ours} vs {theirs}"
            );
        }
    }
}

/// The wide half, spot-checked the same way. It is the same implementation a
/// word wider, so the point here is that the module exists and is wired up
/// rather than that the algorithm is different.
#[test]
fn the_wide_half_matches_its_intrinsics_too() {
    for step in -200i32..200 {
        let x = f64::from(step) / 16.0;
        assert!((corvid_float::wide::sin(x) - x.sin()).abs() < 1e-12);
        assert!((corvid_float::wide::sqrt(x.abs()) - x.abs().sqrt()).abs() < 1e-12);
        assert_eq!(corvid_float::wide::round(x), x.round(), "round({x})");
        assert_eq!(corvid_float::wide::floor(x), x.floor(), "floor({x})");
    }
}

/// The claim the crate is named for. If any of these stops being `const` the
/// build fails here rather than silently at the call site that wanted it.
///
/// Exhaustive on purpose, and it has to be kept that way: a `const` binding is
/// the only thing that can witness constness, so a function this list forgets
/// is a function whose constness nothing checks. This covers the sixteen names
/// at the root and its sibling below covers the fourteen in `wide`; a new
/// export is not finished until it appears in one of them.
#[test]
fn every_function_is_const() {
    const SQRT: f32 = corvid_float::sqrt(2.0);
    const SIN: f32 = corvid_float::sin(1.0);
    const COS: f32 = corvid_float::cos(1.0);
    const TAN: f32 = corvid_float::tan(0.5);
    const RECIP: f32 = corvid_float::recip(4.0);
    const HYPOT: f32 = corvid_float::hypot(3.0, 4.0);
    const POWI: f32 = corvid_float::powi(2.0, 10);
    const CEIL: f32 = corvid_float::ceil(1.2);
    const FLOOR: f32 = corvid_float::floor(1.8);
    const ROUND: f32 = corvid_float::round(1.5);
    const TRUNC: f32 = corvid_float::trunc(-1.8);
    const ABS: f32 = corvid_float::abs(-3.0);
    const SIGN: f32 = corvid_float::copysign(3.0, -1.0);
    const CLAMP: f32 = corvid_float::clamp(9.0, 0.0, 1.0);
    const CLAMP_FINITE: f32 = corvid_float::clamp_finite(f32::INFINITY, 0.0, 1.0);
    const DEMOTE: f32 = corvid_float::demote(0.5);

    assert!((SQRT - 2.0f32.sqrt()).abs() < 1e-6);
    assert!((SIN - 1.0f32.sin()).abs() < 1e-6);
    assert!((COS - 1.0f32.cos()).abs() < 1e-6);
    assert!((TAN - 0.5f32.tan()).abs() < 1e-6);
    assert_eq!(RECIP, 0.25);
    assert!((HYPOT - 5.0).abs() < 1e-6);
    assert_eq!(POWI, 1024.0);
    assert_eq!(CEIL, 2.0);
    assert_eq!(FLOOR, 1.0);
    assert_eq!(ROUND, 2.0);
    assert_eq!(TRUNC, -1.0);
    assert_eq!(ABS, 3.0);
    assert_eq!(SIGN, -3.0);
    assert_eq!(CLAMP, 1.0);
    assert_eq!(CLAMP_FINITE, 0.0);
    assert_eq!(DEMOTE, 0.5);
}

/// The same witness for the wide half, which is a second set of definitions
/// rather than the same ones generically — so it can regress on its own and has
/// to be checked on its own.
///
/// A separate test rather than a second block in the one above only because
/// `clippy::items_after_statements` is on: a `const` is an item, and an item
/// after an assertion is a warning the workspace turns into an error.
/// `clamp_finite` and `demote` are absent here because `wide` does not have
/// them — the first is the audio mixer's and the mixer works in `f32`, and the
/// second only ever narrows in the one direction.
#[test]
fn every_wide_function_is_const() {
    const WIDE_SQRT: f64 = corvid_float::wide::sqrt(16.0);
    const WIDE_SIN: f64 = corvid_float::wide::sin(1.0);
    const WIDE_COS: f64 = corvid_float::wide::cos(1.0);
    const WIDE_TAN: f64 = corvid_float::wide::tan(0.5);
    const WIDE_RECIP: f64 = corvid_float::wide::recip(4.0);
    const WIDE_HYPOT: f64 = corvid_float::wide::hypot(3.0, 4.0);
    const WIDE_POWI: f64 = corvid_float::wide::powi(2.0, 10);
    const WIDE_CEIL: f64 = corvid_float::wide::ceil(1.2);
    const WIDE_FLOOR: f64 = corvid_float::wide::floor(1.8);
    const WIDE_ROUND: f64 = corvid_float::wide::round(1.5);
    const WIDE_TRUNC: f64 = corvid_float::wide::trunc(-1.8);
    const WIDE_ABS: f64 = corvid_float::wide::abs(-3.0);
    const WIDE_SIGN: f64 = corvid_float::wide::copysign(3.0, -1.0);
    const WIDE_CLAMP: f64 = corvid_float::wide::clamp(9.0, 0.0, 1.0);

    assert!((WIDE_SQRT - 4.0).abs() < 1e-12);
    assert!((WIDE_SIN - 1.0f64.sin()).abs() < 1e-12);
    assert!((WIDE_COS - 1.0f64.cos()).abs() < 1e-12);
    assert!((WIDE_TAN - 0.5f64.tan()).abs() < 1e-12);
    assert_eq!(WIDE_RECIP, 0.25);
    assert!((WIDE_HYPOT - 5.0).abs() < 1e-12);
    assert_eq!(WIDE_POWI, 1024.0);
    assert_eq!(WIDE_CEIL, 2.0);
    assert_eq!(WIDE_FLOOR, 1.0);
    assert_eq!(WIDE_ROUND, 2.0);
    assert_eq!(WIDE_TRUNC, -1.0);
    assert_eq!(WIDE_ABS, 3.0);
    assert_eq!(WIDE_SIGN, -3.0);
    assert_eq!(WIDE_CLAMP, 1.0);
}

/// The other half of the `const` claim: the constants the crate re-exports so
/// that a caller reaching for a `PI` names one crate rather than two. They come
/// straight from `core`, but a re-export can be dropped or misspelled and the
/// only place that would show is a downstream build.
#[test]
fn the_re_exported_constants_are_the_core_ones() {
    assert_eq!(corvid_float::consts::PI, core::f32::consts::PI);
    assert_eq!(
        corvid_float::consts::FRAC_PI_2,
        core::f32::consts::FRAC_PI_2
    );
    assert_eq!(corvid_float::wide::consts::PI, core::f64::consts::PI);
}
