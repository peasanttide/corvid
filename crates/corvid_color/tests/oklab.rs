//! Oklab against the reference values in the specification it comes from.

#![allow(
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::panic_in_result_fn,
    reason = "tests assert; panicking is the failure mode"
)]

use corvid_color::{LinearRgba, Oklab, Oklch, Rgba8};
use corvid_fixed::{Angle32, Factor32, I2F30, I16F16};

/// How close two components have to be.
///
/// The reference values below are quoted to six decimals, so this is the
/// tolerance the source has rather than one picked to make a test pass. The
/// arithmetic itself is far tighter: `I2F30` resolves 9.3e-10, and the error
/// that survives is the Q30 rounding of the matrix coefficients.
const CLOSE: f64 = 2e-3;

#[track_caller]
fn near(left: I2F30, right: f64, what: &str) {
    let value = left.to_f64();
    assert!((value - right).abs() < CLOSE, "{what}: {value} vs {right}");
}

/// A linear channel, spelled once.
const fn linear(value: f64) -> I16F16 {
    I16F16::from_f64(value)
}

/// White is fully light with no chroma at all.
#[test]
fn white_has_no_chroma() {
    let lab = Oklab::from_linear(LinearRgba::WHITE);
    near(lab.l, 1.0, "l");
    near(lab.a, 0.0, "a");
    near(lab.b, 0.0, "b");
}

/// Black is nothing, and does not divide by zero on the way there.
#[test]
fn black_is_nothing() {
    let lab = Oklab::from_linear(LinearRgba::BLACK);
    near(lab.l, 0.0, "l");
    near(lab.a, 0.0, "a");
    near(lab.b, 0.0, "b");
}

/// The specification's own reference: linear (1, 0, 0) is L 0.627955,
/// a 0.224863, b 0.125846.
#[test]
fn pure_red_matches_the_reference() {
    let lab = Oklab::from_linear(LinearRgba::rgb(linear(1.0), I16F16::ZERO, I16F16::ZERO));
    near(lab.l, 0.627_955, "l");
    near(lab.a, 0.224_863, "a");
    near(lab.b, 0.125_846, "b");
}

/// And linear (0, 1, 0) is L 0.866440, a -0.233888, b 0.179498.
#[test]
fn pure_green_matches_the_reference() {
    let lab = Oklab::from_linear(LinearRgba::rgb(I16F16::ZERO, linear(1.0), I16F16::ZERO));
    near(lab.l, 0.866_440, "l");
    near(lab.a, -0.233_888, "a");
    near(lab.b, 0.179_498, "b");
}

/// And linear (0, 0, 1) is L 0.452014, a -0.032457, b -0.311528.
#[test]
fn pure_blue_matches_the_reference() {
    let lab = Oklab::from_linear(LinearRgba::rgb(I16F16::ZERO, I16F16::ZERO, linear(1.0)));
    near(lab.l, 0.452_014, "l");
    near(lab.a, -0.032_457, "a");
    near(lab.b, -0.311_528, "b");
}

/// The conversion is invertible, which is what says the cube root is good
/// enough. Every 8-bit code, on all three channels at once.
#[test]
fn every_code_survives_the_round_trip() {
    for code in 0u8..=255 {
        let colour = Rgba8::new(code, 255 - code, code.rotate_left(3), 255);
        let there_and_back = Oklab::from_linear(colour.to_linear())
            .to_linear()
            .to_srgb8();
        assert_eq!(there_and_back, colour, "code {code}");
    }
}

/// Coverage is carried through untouched rather than being a fourth axis.
#[test]
fn coverage_is_carried_through() {
    let glass = Rgba8::new(0xE5, 0x78, 0x29, 0x80);
    let lab = Oklab::from_linear(glass.to_linear());
    assert!(
        (lab.alpha.to_f64() - 128.0 / 255.0).abs() < 1e-4,
        "{:?}",
        lab.alpha
    );
    assert_eq!(lab.to_linear().to_srgb8(), glass);
}

/// Half way from red to green does not go through mud, which is the whole
/// reason this module exists.
#[test]
fn the_midpoint_stays_saturated() {
    let red = Oklab::from_linear(Rgba8::opaque_hex(0xFF_00_00).to_linear());
    let green = Oklab::from_linear(Rgba8::opaque_hex(0x00_FF_00).to_linear());
    let middle = red
        .lerp(green, Factor32::from_f64(0.5))
        .to_linear()
        .to_srgb8();

    let brightest = middle.r.max(middle.g).max(middle.b);
    let dimmest = middle.r.min(middle.g).min(middle.b);
    assert!(
        u16::from(brightest) - u16::from(dimmest) > 80,
        "{middle:?} is not a colour"
    );
}

/// Interpolation is exact at both ends.
#[test]
fn the_lerp_endpoints_are_exact() {
    let red = Oklab::from_linear(Rgba8::opaque_hex(0xFF_00_00).to_linear());
    let green = Oklab::from_linear(Rgba8::opaque_hex(0x00_FF_00).to_linear());
    assert_eq!(red.lerp(green, Factor32::ZERO), red);
    assert_eq!(red.lerp(green, Factor32::ONE), green);
}

/// The polar form and the Cartesian one describe the same colour.
#[test]
fn the_polar_form_round_trips() {
    for code in 0u8..=255 {
        let colour = Rgba8::new(code, code.rotate_left(5), 255 - code, 255);
        let lab = Oklab::from_linear(colour.to_linear());
        let back = lab.to_oklch().to_oklab();
        near(back.l, lab.l.to_f64(), "l");
        near(back.a, lab.a.to_f64(), "a");
        near(back.b, lab.b.to_f64(), "b");
    }
}

/// A hue is an angle, so it wraps: a full turn is where it started.
#[test]
fn a_hue_wraps() {
    let here = Oklch::new(
        I2F30::from_f64(0.7),
        I2F30::from_f64(0.15),
        Angle32::from_turns(0.25),
        I16F16::ONE,
    );
    let round = Oklch::new(
        I2F30::from_f64(0.7),
        I2F30::from_f64(0.15),
        Angle32::from_turns(1.25),
        I16F16::ONE,
    );
    assert_eq!(here.to_linear().to_srgb8(), round.to_linear().to_srgb8());
}

/// Grey has no hue to speak of, and asking for one is not a division by zero.
#[test]
fn grey_has_no_chroma() {
    let grey = Oklab::from_linear(Rgba8::rgb(128, 128, 128).to_linear());
    assert!(grey.to_oklch().c.to_f64() < 1e-3, "{:?}", grey.to_oklch().c);
}

/// A wheel of evenly spaced hues at one lightness and one chroma is five
/// distinct colours, none of them black or white. This is the thing the polar
/// form exists to make easy, and it is what a procedural palette is.
#[test]
fn a_wheel_is_five_colours() {
    let wheel: [Rgba8; 5] = [0u32, 1, 2, 3, 4].map(|spoke| {
        let hue = Angle32::from_turns(f64::from(spoke) / 5.0);
        Oklch::new(
            I2F30::from_f64(0.7),
            I2F30::from_f64(0.15),
            hue,
            I16F16::ONE,
        )
        .to_linear()
        .to_srgb8()
    });

    for (index, colour) in wheel.iter().enumerate() {
        assert_ne!(*colour, Rgba8::BLACK, "spoke {index}");
        assert_ne!(*colour, Rgba8::WHITE, "spoke {index}");
        for (other, sibling) in wheel.iter().enumerate().skip(index + 1) {
            assert_ne!(colour, sibling, "spokes {index} and {other}");
        }
    }
}
