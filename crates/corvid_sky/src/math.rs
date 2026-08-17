//! Trigonometry in degrees, and the two ways an angle is folded back into range.
//!
//! Every series this crate evaluates is published in degrees, so the whole
//! crate works in degrees and converts at the call to `libm` rather than at the
//! call to the series. A transcription checked against a printed table is only
//! checkable while the units on the page and the units in the code are the
//! same.

/// Degrees in one radian.
pub(crate) const DEGREES_PER_RADIAN: f64 = 57.295_779_513_082_32;

/// Radians in one degree.
pub(crate) const RADIANS_PER_DEGREE: f64 = 0.017_453_292_519_943_295;

/// Arcseconds in one degree.
pub(crate) const ARCSECONDS_PER_DEGREE: f64 = 3_600.0;

/// The sine of an angle in degrees.
pub(crate) fn sin(degrees: f64) -> f64 {
    libm::sin(degrees * RADIANS_PER_DEGREE)
}

/// The cosine of an angle in degrees.
pub(crate) fn cos(degrees: f64) -> f64 {
    libm::cos(degrees * RADIANS_PER_DEGREE)
}

/// The tangent of an angle in degrees.
pub(crate) fn tan(degrees: f64) -> f64 {
    libm::tan(degrees * RADIANS_PER_DEGREE)
}

/// The arcsine, in degrees, of a value clamped into the domain first.
///
/// The clamp is not defensive dressing. A unit vector rebuilt from three
/// components that each rounded outwards can carry a `z` of `1.0 + 1e-16`, and
/// an unclamped `asin` answers `NaN` for it -- which then travels silently
/// through every angle downstream instead of arriving as a horizon.
pub(crate) fn asin(value: f64) -> f64 {
    libm::asin(value.clamp(-1.0, 1.0)) * DEGREES_PER_RADIAN
}

/// The arccosine, in degrees, of a value clamped into the domain first.
///
/// Clamped for the reason [`asin`] is.
pub(crate) fn acos(value: f64) -> f64 {
    libm::acos(value.clamp(-1.0, 1.0)) * DEGREES_PER_RADIAN
}

/// The two-argument arctangent, in degrees, over the full circle.
pub(crate) fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x) * DEGREES_PER_RADIAN
}

/// An angle folded into `0.0 ..= 360.0`.
pub(crate) fn wrap360(degrees: f64) -> f64 {
    let folded = degrees - 360.0 * libm::floor(degrees / 360.0);
    // `floor` of a very large negative quotient can round the product back onto
    // 360 exactly, so the range is closed at the top rather than half-open. A
    // caller that needs a half-open range subtracts.
    if folded < 0.0 { folded + 360.0 } else { folded }
}

/// An angle folded into `-180.0 ..= 180.0`, which is what a *difference* of two
/// angles has to be before it can be compared against zero or driven to it.
pub(crate) fn wrap180(degrees: f64) -> f64 {
    let folded = wrap360(degrees);
    if folded > 180.0 {
        folded - 360.0
    } else {
        folded
    }
}

/// A polynomial evaluated by Horner's rule, lowest-order coefficient first.
///
/// Every series in this crate is printed as a polynomial in Julian centuries,
/// so writing them out as coefficient lists keeps the code and the page in the
/// same order and makes a transcription error visible as a wrong number rather
/// than as a wrong expression.
pub(crate) fn poly(coefficients: &[f64], x: f64) -> f64 {
    let mut accumulated = 0.0;
    for coefficient in coefficients.iter().rev() {
        accumulated = accumulated * x + coefficient;
    }
    accumulated
}

/// A polynomial whose coefficients are **arcseconds**, answered in degrees.
pub(crate) fn poly_arcseconds(coefficients: &[f64], x: f64) -> f64 {
    poly(coefficients, x) / ARCSECONDS_PER_DEGREE
}

/// The whole part of a value, as an `i64`.
///
/// Rust defines a float-to-integer `as` cast as saturating, with `NaN` mapping
/// to zero, so this has no undefined case and needs no guard. It exists as a
/// named function so that the one lint override the cast needs is written once
/// rather than at every calendar boundary.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a float-to-integer `as` cast saturates in Rust rather than wrapping, and every caller is a Julian day number or a calendar field, all of which are far inside `i64`"
)]
pub(crate) fn whole(value: f64) -> i64 {
    libm::floor(value) as i64
}
