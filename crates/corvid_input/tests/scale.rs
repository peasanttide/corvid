//! Crossing from a normalized axis to a fixed-point quantity.
//!
//! The two scales do not line up -- an axis is `bits / 32767` and every quantity
//! is `bits / 2^n` -- so this crossing is a multiply and a rounded divide, and
//! each test below names one thing a cheaper crossing gets wrong. The two
//! cheaper ones worth naming are `bits >> 15`, which never reaches the top of
//! the range, and a truncating divide, which is asymmetric about zero.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

use corvid_fixed::{I16F16, I24F8, Signed16};
use corvid_input::{Analog, scale, scale_coarse};
use corvid_vector::{FinePoint, GlobalPoint};

/// A scale with bits set low and high, so a conversion that dropped either end
/// of the product is visible.
const FULL: I16F16 = I16F16::from_bits(0x0002_4CCD);

#[test]
fn the_ends_are_exact() {
    // The property a shift does not have. `bits >> 15` maps 32767 to 32767/32768
    // of the scale, so a stick held all the way over is a quantity that is one
    // part in 32768 short of what the game asked for -- forever, on every frame,
    // in an action every peer hashes.
    assert_eq!(scale(Signed16::MAX, FULL), FULL);
    assert_eq!(scale(Signed16::MIN, FULL), -FULL);
    assert_eq!(scale(Signed16::ZERO, FULL), I16F16::ZERO);

    // And the shift really is what it is being read against, so this is a
    // comparison rather than an assertion about nothing: the shifted answer is
    // a different number.
    let shifted =
        I16F16::from_bits(i32::try_from((i64::from(FULL.to_bits()) >> 15) * 32767).unwrap());
    assert_ne!(shifted, FULL);
}

#[test]
fn the_scale_is_not_the_only_thing_the_ends_are_exact_for() {
    // Every scale, not the one this file happens to name. A crossing that was
    // exact at one scale and off by a bit at another would pass the test above.
    for bits in [1, 2, 3, 255, 256, 65_535, 65_536, i32::MAX] {
        let full = I16F16::from_bits(bits);
        assert_eq!(scale(Signed16::MAX, full), full, "{bits}");
        assert_eq!(scale(Signed16::MIN, full), -full, "{bits}");
    }
}

#[test]
fn a_push_left_is_a_push_right_reversed() {
    // The rounding is what makes this true at every axis, and the mistake it is
    // fitted against is a floor -- `numerator.div_euclid(denominator)`, which is
    // one line and reads like a divide. Under it a push left is one step
    // shorter than the same push right, on every frame, in an action every peer
    // hashes. A truncating divide survives this one and is caught below, which
    // is why both tests are here.
    for bits in (-32_767..=32_767).step_by(37) {
        let axis = Signed16::from_bits(bits);
        assert_eq!(scale(-axis, FULL), -scale(axis, FULL), "{bits}");
    }
}

#[test]
fn the_denormal_is_not_a_step_outside_the_range() {
    // `SNORM` spends one bit pattern twice: -32768 and -32767 both mean -1.0.
    // A conversion that multiplied the raw bits would answer one step past the
    // negative end for the first of them, which is a quantity the game said was
    // impossible.
    let denormal = Signed16::from_bits(-32_768);
    assert_eq!(scale(denormal, FULL), -FULL);
    assert_eq!(scale(denormal, FULL), scale(Signed16::MIN, FULL));
}

#[test]
fn a_bigger_push_is_never_a_smaller_quantity() {
    // Monotonic, which a conversion that rounded on the magnitude and then
    // negated would still be, and one that mixed two rounding rules across zero
    // would not.
    let mut last = scale(Signed16::MIN, FULL);
    for bits in (-32_767..=32_767).step_by(13) {
        let now = scale(Signed16::from_bits(bits), FULL);
        assert!(now >= last, "{bits}: {now:?} came after {last:?}");
        last = now;
    }
}

#[test]
fn the_quantity_is_the_product_and_not_a_lookup() {
    // Half the stick is half the quantity, to within the rounding -- checked
    // against an independent computation in `i128`, so this is not the
    // implementation compared with itself.
    for bits in [1_i64, 1000, 16_384, 30_000, 32_766] {
        let expected = {
            let numerator = i128::from(bits) * i128::from(FULL.to_bits());
            let denominator = i128::from(Signed16::MAX.to_bits());
            i64::try_from((2 * numerator + denominator) / (2 * denominator)).unwrap()
        };
        assert_eq!(
            scale(Signed16::from_bits(i16::try_from(bits).unwrap()), FULL).to_bits(),
            i32::try_from(expected).unwrap(),
            "{bits}",
        );
    }
}

#[test]
fn the_coarse_tier_crosses_the_same_way() {
    let full = I24F8::from_f64(100.0);
    assert_eq!(scale_coarse(Signed16::MAX, full), full);
    assert_eq!(scale_coarse(Signed16::MIN, full), -full);
    assert_eq!(scale_coarse(Signed16::ZERO, full), I24F8::ZERO);

    // The widest scale the tier has, where a crossing that stayed in 32 bits
    // overflows: 2^31 times 32767 needs 46 bits.
    let widest = I24F8::from_bits(i32::MAX);
    assert_eq!(scale_coarse(Signed16::MAX, widest), widest);
}

#[test]
fn a_stick_maps_to_the_ground_plane() {
    // +X right, +Y forward, +Z up, which is the workspace's convention and the
    // only thing a caller has to know to read the result.
    let full = I16F16::from_f64(0.25);
    let forward = Analog::new(Signed16::ZERO, Signed16::MAX);
    let right = Analog::new(Signed16::MAX, Signed16::ZERO);

    assert_eq!(
        forward.on_the_ground(full),
        FinePoint::new(I16F16::ZERO, full, I16F16::ZERO),
    );
    assert_eq!(
        right.on_the_ground(full),
        FinePoint::new(full, I16F16::ZERO, I16F16::ZERO),
    );

    // Both axes at once, so the two are not being read from one slot.
    let corner = Analog::new(Signed16::MAX, Signed16::MIN);
    assert_eq!(
        corner.on_the_ground(full),
        FinePoint::new(full, -full, I16F16::ZERO),
    );

    let coarse = I24F8::from_f64(3.0);
    assert_eq!(
        Analog::new(Signed16::MIN, Signed16::MAX).on_the_ground_coarse(coarse),
        GlobalPoint::new(-coarse, coarse, I24F8::ZERO),
    );
}

#[test]
fn a_centred_stick_is_a_standing_still() {
    // The value a snapshot answers with for an action nobody is touching, and
    // for an action outside the active set. A crossing with an offset in it
    // would drift a game in whatever direction it leaned.
    assert_eq!(
        Analog::ZERO.on_the_ground(I16F16::from_f64(1000.0)),
        FinePoint::ZERO,
    );
}

/// The one place the scaling clamps, and it is the type's asymmetry rather
/// than the arithmetic's.
///
/// A two's-complement range holds one more negative than positive, so `-MIN` is
/// not a value. `scale` promises that `MIN` gives `-full`, and for a `full` of
/// `MIN` there is no such quantity -- so it answers the nearest one that
/// exists. Worth pinning rather than leaving to the reader, because the
/// alternative to a clamp here is a wrap, and a wrap turns a stick pushed fully
/// one way into a full push the other.
#[test]
fn the_one_scale_with_no_negation_saturates_rather_than_wrapping() {
    assert_eq!(scale(Signed16::MIN, I16F16::MIN), I16F16::MAX);
    assert_eq!(scale_coarse(Signed16::MIN, I24F8::MIN), I24F8::MAX);

    // Its neighbour has a negation, and gets it exactly.
    let almost = I16F16::from_bits(I16F16::MIN.to_bits() + 1);
    assert_eq!(scale(Signed16::MIN, almost), I16F16::MAX);
    assert_eq!(scale(Signed16::MAX, almost), almost);

    // And the positive end is unaffected, which is what makes this the range's
    // asymmetry rather than a fault in the rounding.
    assert_eq!(scale(Signed16::MAX, I16F16::MIN), I16F16::MIN);
    assert_eq!(scale_coarse(Signed16::MAX, I24F8::MIN), I24F8::MIN);
}
