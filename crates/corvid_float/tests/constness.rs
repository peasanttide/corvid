//! That every exported function is callable in a `const`.
//!
//! This is the crate's whole reason to exist, and a `const` initializer is the
//! only thing that can witness it: an intrinsic compiles fine in a runtime call
//! and fails only here. So the lists below name every export, and a function
//! one of them forgets is a function whose constness nothing checks.

#![allow(
    clippy::float_cmp,
    reason = "the assertion is that a const initializer produced a particular value, and the values are the exact ones the constants carry"
)]

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
/// rather than the same ones generically -- so it can regress on its own and has
/// to be checked on its own.
///
/// A separate test rather than a second block in the one above only because
/// `clippy::items_after_statements` is on: a `const` is an item, and an item
/// after an assertion is a warning the workspace turns into an error.
/// `clamp_finite` and `demote` are absent here because `wide` does not have
/// them -- the first is the audio mixer's and the mixer works in `f32`, and the
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
