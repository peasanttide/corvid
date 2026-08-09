//! The clamping quarter-turn angles: what they hold, and what they refuse.
//!
//! A pitch clamps where an angle wraps, so the redundant bit patterns outside
//! the range are the subject: they read as clamped, they compare as clamped,
//! and they convert to the wrapping angle as clamped.

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
mod common;

use corvid_fixed::{
    Angle8, Angle16, Angle32, I24F8, Pitch8, Pitch16, Pitch32, Signed8, Signed16, Signed32,
};
#[test]
fn the_endpoints_are_exactly_a_quarter_turn() {
    assert_eq!(Pitch16::MAX.to_degrees(), 90.0);
    assert_eq!(Pitch16::MIN.to_degrees(), -90.0);
    assert_eq!(Pitch16::MAX.to_turns(), 0.25);
    assert_eq!(Pitch16::MIN.to_turns(), -0.25);
    assert_eq!(Pitch16::MAX.to_radians(), core::f64::consts::FRAC_PI_2);
    assert_eq!(Pitch16::MIN.to_radians(), -core::f64::consts::FRAC_PI_2);

    assert_eq!(Pitch8::MAX.to_degrees(), 90.0);
    assert_eq!(Pitch32::MAX.to_degrees(), 90.0);
    assert_eq!(Pitch8::MAX.to_bits(), 64);
    assert_eq!(Pitch16::MAX.to_bits(), 16_384);
    assert_eq!(Pitch32::MAX.to_bits(), 1_073_741_824);

    // Symmetric, unlike two's complement, so negation is always exact.
    assert_eq!(-Pitch16::MAX, Pitch16::MIN);
    assert_eq!(-Pitch16::MIN, Pitch16::MAX);
    assert_eq!(Pitch16::MIN.abs(), Pitch16::MAX);
}

#[test]
fn conversion_clamps_rather_than_wrapping() {
    // The whole point of the type: no matter how far you look up, you are looking
    // up, never suddenly down.
    assert_eq!(Pitch16::from_degrees(91.0), Pitch16::MAX);
    assert_eq!(Pitch16::from_degrees(180.0), Pitch16::MAX);
    assert_eq!(Pitch16::from_degrees(1e9), Pitch16::MAX);
    assert_eq!(Pitch16::from_degrees(-91.0), Pitch16::MIN);
    assert_eq!(Pitch16::from_degrees(-1e9), Pitch16::MIN);
    assert_eq!(Pitch16::from_turns(f64::INFINITY), Pitch16::MAX);
    assert_eq!(Pitch16::from_turns(f64::NEG_INFINITY), Pitch16::MIN);
    assert_eq!(Pitch16::from_turns(f64::NAN), Pitch16::ZERO);

    // Compare with the wrapping angle, which does the opposite.
    assert_eq!(Angle16::from_degrees(180.0), Angle16::HALF_TURN);

    assert_eq!(Pitch16::checked_from_f64(0.3), None);
    assert_eq!(Pitch16::checked_from_f64(0.25), Some(Pitch16::MAX));
    assert_eq!(Pitch16::checked_from_f64(-0.25), Some(Pitch16::MIN));
    assert_eq!(Pitch16::checked_from_f64(f64::NAN), None);
}

#[test]
fn arithmetic_saturates_at_the_poles() {
    let mut pitch = Pitch16::from_degrees(85.0);
    pitch += Pitch16::from_degrees(20.0);
    assert_eq!(pitch, Pitch16::MAX);

    // An operand clamps on the way in, so 200 degrees is 90 degrees, and
    // subtracting it from 90 lands on zero rather than overshooting.
    pitch -= Pitch16::from_degrees(200.0);
    assert_eq!(pitch, Pitch16::ZERO);

    // The result saturates too: two more quarter turns down stops at the bottom.
    pitch -= Pitch16::MAX;
    pitch -= Pitch16::MAX;
    assert_eq!(pitch, Pitch16::MIN);

    assert_eq!(Pitch16::MAX + Pitch16::DELTA, Pitch16::MAX);
    assert_eq!(Pitch16::MIN - Pitch16::DELTA, Pitch16::MIN);
    assert_eq!(Pitch16::MAX.checked_add(Pitch16::DELTA), None);
    assert_eq!(Pitch16::MIN.checked_sub(Pitch16::DELTA), None);
    assert_eq!(Pitch16::MAX.checked_add(Pitch16::ZERO), Some(Pitch16::MAX));
    assert_eq!(Pitch16::MAX.checked_sub(Pitch16::MAX), Some(Pitch16::ZERO));
}

#[test]
fn out_of_range_bit_patterns_read_as_clamped() {
    // from_bits stays the exact inverse of to_bits, so bytemuck and serde are
    // faithful, but nothing outside the range is treated as being outside it.
    let raw = Pitch16::from_bits(30_000);
    assert!(raw.is_out_of_range());
    assert_eq!(raw.to_bits(), 30_000);
    assert_eq!(raw, Pitch16::MAX);
    assert_eq!(raw.to_degrees(), 90.0);
    assert_eq!(raw.canonicalize(), Pitch16::MAX);
    assert_eq!(raw.canonicalize().to_bits(), 16_384);
    assert_eq!(raw.sin(), Signed16::MAX);

    let low = Pitch16::from_bits(i16::MIN);
    assert_eq!(low, Pitch16::MIN);
    assert_eq!(low.to_degrees(), -90.0);
    assert!(!Pitch16::MAX.is_out_of_range());
}

#[test]
fn min_max_and_clamp_return_canonical_bits() {
    // `clamp` promises a value in `min ..= max`, which has to mean the bits and
    // not merely something that compares equal to them: a caller who clamps a
    // deserialized pitch and then hands `to_bits()` to a wire or a vertex buffer
    // would otherwise pass on a pattern the type does not mean.
    let raw = Pitch16::from_bits(20_000);
    assert!(raw.is_out_of_range());
    for result in [
        raw.clamp(Pitch16::MIN, Pitch16::MAX),
        raw.min(Pitch16::MAX),
        raw.max(Pitch16::MIN),
        raw.min(raw),
    ] {
        assert!(
            !result.is_out_of_range(),
            "left {} in range",
            result.to_bits()
        );
        assert_eq!(result.to_bits(), Pitch16::MAX.to_bits());
    }

    // Every width, and the low end too -- through `min` and `max` directly as
    // well as through the `clamp` built on them.
    let low8 = Pitch8::from_bits(i8::MIN);
    assert_eq!(low8.clamp(Pitch8::MIN, Pitch8::MAX).to_bits(), -64);
    assert_eq!(low8.max(Pitch8::MIN).to_bits(), -64);
    assert_eq!(low8.min(Pitch8::MAX).to_bits(), -64);

    let high32 = Pitch32::from_bits(i32::MAX);
    assert_eq!(
        high32.clamp(Pitch32::MIN, Pitch32::MAX).to_bits(),
        1_073_741_824
    );
    assert_eq!(high32.min(Pitch32::MAX).to_bits(), 1_073_741_824);
    assert_eq!(high32.max(Pitch32::MIN).to_bits(), 1_073_741_824);

    // A reversed range still returns `max`, and still returns it canonically.
    assert_eq!(
        raw.clamp(Pitch16::MAX, Pitch16::MIN).to_bits(),
        Pitch16::MIN.to_bits()
    );
}

#[test]
fn the_ord_trait_path_canonicalizes_too() {
    // Generic code reaches `Ord`'s provided methods, not the inherent ones, so
    // those have to be overridden or the whole guarantee leaks out through a
    // helper that never mentions a pitch at all.
    fn tighten<T: Ord>(value: T, low: T, high: T) -> T {
        value.clamp(low, high)
    }
    fn lesser<T: Ord>(a: T, b: T) -> T {
        a.min(b)
    }
    fn greater<T: Ord>(a: T, b: T) -> T {
        a.max(b)
    }

    let raw = Pitch16::from_bits(30_000);
    assert!(raw.is_out_of_range());
    assert_eq!(tighten(raw, Pitch16::MIN, Pitch16::MAX).to_bits(), 16_384);
    assert_eq!(lesser(raw, Pitch16::MAX).to_bits(), 16_384);
    assert_eq!(greater(raw, Pitch16::MIN).to_bits(), 16_384);

    // `Ord::clamp`'s default asserts `low <= high`; ours cannot panic.
    assert_eq!(
        tighten(raw, Pitch16::MAX, Pitch16::MIN).to_bits(),
        Pitch16::MIN.to_bits()
    );

    // The signed family's denormal folds by the same route.
    let denormal = Signed8::from_bits(i8::MIN);
    assert!(denormal.is_denormal());
    assert_eq!(lesser(denormal, Signed8::MAX).to_bits(), -127);
    assert_eq!(greater(denormal, Signed8::MIN).to_bits(), -127);
    assert_eq!(
        tighten(denormal, Signed8::MIN, Signed8::MAX).to_bits(),
        -127
    );

    // The edge of the guarantee: selecting an element rather than computing a
    // result hands back what it was given. `Iterator::max` compares with `cmp`
    // and returns the element, so a raw pattern survives -- the caller wanting
    // canonical bits asks for them.
    let picked = [raw, Pitch16::ZERO].into_iter().max();
    assert_eq!(picked.map(Pitch16::to_bits), Some(30_000));
    assert_eq!(picked.map(|v| v.canonicalize().to_bits()), Some(16_384));
}

#[test]
fn trigonometry_spans_the_full_output_range() {
    assert_eq!(Pitch16::MAX.sin(), Signed16::MAX);
    assert_eq!(Pitch16::MIN.sin(), Signed16::MIN);
    assert_eq!(Pitch16::ZERO.sin(), Signed16::ZERO);
    assert_eq!(Pitch16::MAX.cos(), Signed16::ZERO);
    assert_eq!(Pitch16::MIN.cos(), Signed16::ZERO);
    assert_eq!(Pitch16::ZERO.cos(), Signed16::MAX);

    assert_eq!(Pitch8::MAX.sin(), Signed8::MAX);
    assert_eq!(Pitch32::MAX.sin(), Signed32::MAX);
    assert_eq!(Pitch32::MIN.cos(), Signed32::ZERO);
}

#[test]
fn the_cosine_of_a_pitch_is_never_negative() {
    // What makes a pitch safe to build a direction vector from: the horizontal
    // component never flips sign behind your back.
    for bits in i16::MIN..=i16::MAX {
        let pitch = Pitch16::from_bits(bits);
        assert!(!pitch.cos().is_negative(), "cos went negative at {bits}");
    }
}

#[test]
fn pitch_trigonometry_matches_the_angle_it_shares_a_scale_with() {
    for bits in -16_384_i16..=16_384 {
        let pitch = Pitch16::from_bits(bits);
        let angle = pitch.to_angle();
        assert_eq!(pitch.sin(), angle.sin(), "sin differs at {bits}");
        assert_eq!(pitch.cos(), angle.cos(), "cos differs at {bits}");
        assert_eq!(pitch.tan(), angle.tan(), "tan differs at {bits}");
        assert_eq!(
            pitch.sin_fast(),
            angle.sin_fast(),
            "sin_fast differs at {bits}"
        );
        assert_eq!(
            pitch.cos_fast(),
            angle.cos_fast(),
            "cos_fast differs at {bits}"
        );
        assert_eq!(pitch.sin_cos(), (pitch.sin(), pitch.cos()));
    }
}

#[test]
fn tangent_saturates_at_the_poles() {
    assert_eq!(Pitch16::MAX.tan(), I24F8::MAX);
    assert_eq!(Pitch16::MIN.tan(), I24F8::MIN);
    assert_eq!(Pitch16::ZERO.tan(), I24F8::ZERO);
    // Half way up is 45 degrees, where the tangent is one.
    assert_eq!(Pitch16::from_degrees(45.0).tan(), I24F8::ONE);
    assert_eq!(Pitch16::from_degrees(-45.0).tan(), -I24F8::ONE);
}

#[test]
fn converting_to_and_from_a_wrapping_angle_is_free() {
    for bits in -16_384_i16..=16_384 {
        let pitch = Pitch16::from_bits(bits);
        assert_eq!(
            Pitch16::from_angle(pitch.to_angle()),
            pitch,
            "round trip at {bits}"
        );
        assert_eq!(
            pitch.to_angle().to_bits(),
            bits as u16,
            "bits changed at {bits}"
        );
    }

    // Angles beyond the range read as signed offsets, then clamp.
    assert_eq!(
        Pitch16::from_angle(Angle16::from_degrees(350.0))
            .to_degrees()
            .round(),
        -10.0
    );
    assert_eq!(
        Pitch16::from_angle(Angle16::from_degrees(120.0)),
        Pitch16::MAX
    );
    assert_eq!(
        Pitch16::from_angle(Angle16::from_degrees(240.0)),
        Pitch16::MIN
    );
    assert_eq!(Pitch8::from_angle(Angle8::ZERO), Pitch8::ZERO);
    assert_eq!(Pitch32::from_angle(Angle32::QUARTER_TURN), Pitch32::MAX);
}

#[test]
fn to_angle_reads_the_clamped_value_not_the_raw_bits() {
    // In range it is the identity on the bit pattern, which is the whole point
    // of sharing a scale with the wrapping angle.
    for bits in -16_384_i16..=16_384 {
        assert_eq!(Pitch16::from_bits(bits).to_angle().to_bits(), bits as u16);
    }

    // Out of range it reads the pitch's *value*, so it cannot hand on a phase
    // the type does not mean. `to_bits` is what round-trips raw bytes.
    let raw = Pitch16::from_bits(30_000);
    assert_eq!(
        raw.to_bits(),
        30_000,
        "to_bits still reports what it was handed"
    );
    assert_eq!(raw.to_angle(), Pitch16::MAX.to_angle());
    assert_eq!(raw.to_angle().to_bits(), 16_384);
    assert_eq!(
        Pitch16::from_bits(i16::MIN).to_angle(),
        Pitch16::MIN.to_angle()
    );
    assert_eq!(Pitch8::from_bits(100).to_angle().to_bits(), 64);

    // Which keeps to_angle agreeing with the trigonometry, since both read the
    // clamped value.
    assert_eq!(raw.sin(), raw.to_angle().sin());
}
