//! Conversions between this crate's own types, rather than to and from a
//! float.
//!
//! Two shapes, and the difference between them is the whole subject. The
//! signed normalized widths all denote `-1.0 ..= 1.0` and differ only in how
//! finely they cut it, so widening is exact and narrowing rounds. The
//! fixed-point types trade range against fraction instead, so a conversion
//! between them moves both and one direction of each pair can clamp.
//!
//! What every one of them owes is its ends: a full deflection that came back
//! one step short would turn a stick held hard over into a stick almost held
//! over, and these sit on the path from a control to a value.

#![allow(
    clippy::float_cmp,
    reason = "comparisons are against exactly representable references"
)]

use corvid_fixed::{I16F16, I24F8, Signed8, Signed16, Signed32};

/// The signed normalized widths convert both ways, exactly where they can.
///
/// The ends matter more than the middle here: a full deflection that came back
/// one step short of full would turn a stick held hard over into a stick almost
/// held over, and every one of these is on the path from a control to a value.
#[test]
fn the_signed_widths_convert_both_ways() {
    // Widening is exact, so a round trip through it is the identity.
    for bits in [0i16, 1, -1, 1000, -1000, i16::MAX, -i16::MAX] {
        let value = Signed16::from_bits(bits);
        assert_eq!(value.to_signed32().to_signed16(), value, "{value:?}");
    }

    // The ends survive in both directions.
    assert_eq!(Signed32::MAX.to_signed16(), Signed16::MAX);
    assert_eq!(Signed32::MIN.to_signed16(), Signed16::MIN);
    assert_eq!(Signed16::MAX.to_signed32(), Signed32::MAX);
    assert_eq!(Signed16::MIN.to_signed32(), Signed32::MIN);
    assert_eq!(Signed8::MAX.to_signed32(), Signed32::MAX);
    assert_eq!(Signed32::MAX.to_signed8(), Signed8::MAX);

    // Zero is zero at every width, and the denormal is folded on the way.
    assert_eq!(Signed32::ZERO.to_signed16(), Signed16::ZERO);
    assert_eq!(Signed16::from_bits(i16::MIN).to_signed32(), Signed32::MIN);
}

/// Narrowing rounds to nearest rather than truncating, on both sides of zero.
#[test]
fn narrowing_a_signed_width_rounds() {
    // Just over half a step of the destination goes away from zero.
    let step = f64::from(i32::MAX) / f64::from(i16::MAX);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the value is a fraction of i32::MAX by construction"
    )]
    let just_over = Signed32::from_bits((step * 0.6) as i32);
    assert_eq!(just_over.to_signed16().to_bits(), 1);
    assert_eq!((-just_over).to_signed16().to_bits(), -1);

    // And just under stays at zero.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the value is a fraction of i32::MAX by construction"
    )]
    let just_under = Signed32::from_bits((step * 0.4) as i32);
    assert_eq!(just_under.to_signed16(), Signed16::ZERO);
}

/// The whole `I16F16`/`I24F8` pair, in the direction a mesh uses.
#[test]
fn narrowing_the_fraction_widens_the_range() {
    for value in [0.0, 1.5, -1.5, 1000.0, -1000.0] {
        let fine = I16F16::from_f64(value);
        assert_eq!(fine.to_i24f8().to_f64(), value, "{value}");
    }

    // Rounded rather than truncated: half a step of the destination.
    assert_eq!(I16F16::from_bits(1 << 7).to_i24f8(), I24F8::from_bits(1));
    assert_eq!(
        I16F16::from_bits(-(1 << 7)).to_i24f8(),
        I24F8::from_bits(-1)
    );

    // It cannot fail -- `I16F16`'s whole range is inside `I24F8`'s -- but it
    // does round, and at the ends that is the visible half of the operation:
    // `I16F16::MAX` is 32767.99998, which is nearer 32768 than to anything
    // else eight fractional bits can say.
    let half_step = 1.0 / 512.0;
    for end in [I16F16::MAX, I16F16::MIN] {
        let moved = end.to_i24f8().to_f64() - end.to_f64();
        assert!(moved.abs() <= half_step, "{end:?} moved by {moved}");
    }
    assert_eq!(I16F16::MAX.to_i24f8().to_f64(), 32768.0);
}
