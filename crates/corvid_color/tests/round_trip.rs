//! The transfer function is exact in both directions, for every code.

#![allow(
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::panic_in_result_fn,
    reason = "tests assert; panicking is the failure mode"
)]

use corvid_color::{LinearRgba, Rgba8, decode, encode};
use corvid_fixed::{Factor32, I16F16};

/// Every one of the 256 codes decodes to a linear value that encodes back to
/// the code it came from.
///
/// This is the property the whole module exists for. A transfer function that
/// is merely close round-trips two hundred and fifty-four of them and moves one
/// golden PNG by one least-significant bit, on one driver, once.
#[test]
fn every_code_round_trips() {
    for code in 0u8..=255 {
        assert_eq!(encode(decode(code)), code, "code {code}");
    }
}

/// The two ends are exact rather than nearly so.
#[test]
fn the_endpoints_are_exact() {
    assert_eq!(decode(0), I16F16::ZERO);
    assert_eq!(decode(255), I16F16::ONE);
}

/// The function is monotonic, which is what makes the binary search in
/// `encode` correct rather than merely usually right.
#[test]
fn decoding_is_monotonic() {
    for code in 0u8..255 {
        assert!(decode(code) < decode(code + 1), "code {code}");
    }
}

/// Mid grey in sRGB is not mid grey in light, and getting that backwards is the
/// single most common colour bug there is. 128 decodes to about 0.2158.
#[test]
fn mid_grey_is_not_half() {
    let mid = decode(128).to_f64();
    assert!((0.215..0.217).contains(&mid), "{mid}");
}

/// And the other way: half the light is a good deal brighter than the sRGB code
/// half way up.
#[test]
fn half_the_light_is_not_mid_grey() {
    assert_eq!(encode(I16F16::from_f64(0.5)), 188);
}

/// A colour survives the trip through linear and back.
#[test]
fn a_colour_round_trips_through_linear() {
    let orange = Rgba8::opaque_hex(0xE5_78_29);
    assert_eq!(orange.to_linear().to_srgb8(), orange);
}

/// Every colour does, on all three channels at once, at every code.
#[test]
fn every_colour_round_trips_through_linear() {
    for code in 0u8..=255 {
        let colour = Rgba8::new(code, 255 - code, code.rotate_left(3), code / 2);
        assert_eq!(colour.to_linear().to_srgb8(), colour, "code {code}");
    }
}

/// Alpha is not transferred. It is a coverage fraction, not a light level, so
/// running it through the transfer function would darken every soft edge in the
/// game.
#[test]
fn alpha_is_linear_already() {
    let half = Rgba8::new(0, 0, 0, 128);
    let linear = half.to_linear();
    assert!(
        (linear.a.to_f64() - 128.0 / 255.0).abs() < 1e-4,
        "{:?}",
        linear.a
    );
}

/// The constants agree across the two representations.
#[test]
fn the_constants_agree() {
    assert_eq!(Rgba8::WHITE.to_linear(), LinearRgba::WHITE);
    assert_eq!(Rgba8::BLACK.to_linear(), LinearRgba::BLACK);
    assert_eq!(Rgba8::TRANSPARENT.to_linear(), LinearRgba::TRANSPARENT);
}

/// A value outside the unit range clamps rather than wrapping, and there is no
/// third case: a fixed-point channel has no `NaN`, which is one of the things
/// moving this crate off floating point bought.
#[test]
fn the_edges_clamp() {
    assert_eq!(encode(I16F16::from_f64(-1.0)), 0);
    assert_eq!(encode(I16F16::from_f64(2.0)), 255);
    assert_eq!(encode(I16F16::MIN), 0);
    assert_eq!(encode(I16F16::MAX), 255);

    let nonsense = LinearRgba::new(
        I16F16::from_f64(-5.0),
        I16F16::ZERO,
        I16F16::from_f64(12.0),
        I16F16::from_f64(3.0),
    );
    assert_eq!(nonsense.to_srgb8(), Rgba8::new(0, 0, 255, 255));
}

/// A linear colour compares and orders, which an `f32` one could not — and it
/// is the property that lets one reach a golden.
#[test]
fn a_linear_colour_is_totally_ordered() {
    assert!(LinearRgba::BLACK < LinearRgba::WHITE);
    assert_eq!(LinearRgba::WHITE, Rgba8::WHITE.to_linear());
}

/// Interpolation is exact at both ends, which is what every interpolation in
/// this workspace owes.
#[test]
fn the_lerp_endpoints_are_exact() {
    let from = LinearRgba::new(
        I16F16::from_f64(0.1),
        I16F16::from_f64(0.2),
        I16F16::from_f64(0.3),
        I16F16::from_f64(0.4),
    );
    let to = LinearRgba::new(
        I16F16::from_f64(0.9),
        I16F16::from_f64(0.8),
        I16F16::from_f64(0.7),
        I16F16::from_f64(0.6),
    );
    assert_eq!(from.lerp(to, Factor32::ZERO), from);
    assert_eq!(from.lerp(to, Factor32::ONE), to);
}

/// Premultiplying folds the coverage in and leaves it in place.
#[test]
fn premultiplying_folds_the_coverage_in() {
    let glass = LinearRgba::new(
        I16F16::ONE,
        I16F16::from_f64(0.5),
        I16F16::ZERO,
        I16F16::from_f64(0.5),
    );
    assert_eq!(
        glass.premultiplied(),
        LinearRgba::new(
            I16F16::from_f64(0.5),
            I16F16::from_f64(0.25),
            I16F16::ZERO,
            I16F16::from_f64(0.5),
        )
    );
}
