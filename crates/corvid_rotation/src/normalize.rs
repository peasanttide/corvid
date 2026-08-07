//! The one normalize every decoder in this crate shares.
//!
//! Both packed codecs end in the same question — given four integers, what unit
//! quaternion do they point at? — and the answer is the dominant cost of either
//! decode. It is worth writing once, and worth writing without a division.
//!
//! Normalizing is **scale-free**: only the ratios of the inputs matter, so a
//! codec hands over its raw fields and never divides by its own denominator
//! first.

use corvid_fixed::I2F30;

/// `1.0` at the Q30 scale `I2F30` uses.
const ONE: i64 = 1 << 30;

/// Normalizes a 4-vector given at any common scale into `I2F30` bit patterns.
///
/// Returns the **identity** for an all-zero input. Zero is not a quaternion and
/// has no direction to normalize, and returning it unchanged would hand every
/// caller a value that is not a rotation at all: `Versor::from_xyzw` rejects it,
/// `renormalize` cannot repair it, `to_basis` reads it as the identity while
/// `angle_to` reads it as a half turn, and `compose` spreads it. The inputs that
/// reach it are all ones where the identity is the honest answer — an all-zero
/// `FineRotation` off a zeroed buffer, `from_axis_angle` about a zero axis, a
/// `mint` quaternion that collapsed in `f64`.
///
/// # How the range works out
///
/// This is the reduction [`I2F30`] was given its spare bit of range for.
/// Rescaled so the largest component is just under `1`, the sum of squares
/// lands in `[0.25, 4]` — which does not fit a type that stops at `±2`. But an
/// **even** shift always brings it into `[0.25, 1]`, and `rsqrt` of *that*
/// lands in `[1, 2]`, which fits exactly. Undoing the shift afterwards is
/// another shift, because the shift was even.
///
/// The whole routine is one [`rsqrt`](I2F30::rsqrt), four multiplies, and a
/// handful of shifts. There is no division anywhere.
#[must_use]
#[inline]
pub(crate) const fn normalize4(v: [i64; 4]) -> [i32; 4] {
    normalize4_tier(v, false)
}

/// [`normalize4`] over the approximate reciprocal square root.
///
/// Identical in every step but one: the reduced sum of squares goes through
/// [`rsqrt_fast`](I2F30::rsqrt_fast) rather than [`rsqrt`](I2F30::rsqrt),
/// trading that step's exact rounding for roughly 3.7x its throughput.
///
/// The `3.2e-5` relative error scales all four components alike, so it carries
/// straight through step 4: each lands within about `2^15` of its Q30 value —
/// four decimal digits of a unit quaternion, against the twelve [`normalize4`]
/// gives. That is well inside what either packed codec resolves.
///
/// The error being *common-mode* is what makes it cheap here: it moves the
/// norm, not the direction, so the rotation this names is far closer to the
/// exact tier's than the component figures suggest.
///
/// Zero still returns the identity, and the reduction's `0.25` case still lands
/// on an exact `±1` by hand, so the axis-aligned rotations stay exact in both
/// tiers. What the approximation does cost a repeated caller is a deadband —
/// see [`renormalize_fast`](crate::Versor::renormalize_fast), where it is the
/// dominant effect.
#[must_use]
#[inline]
pub(crate) const fn normalize4_fast(v: [i64; 4]) -> [i32; 4] {
    normalize4_tier(v, true)
}

/// The body of both normalizes, which differ only in which `rsqrt` they call.
///
/// `fast` is a literal at both call sites, so the branch in step 3 folds away
/// and neither tier pays for the other's existence.
#[must_use]
#[inline]
const fn normalize4_tier(v: [i64; 4], fast: bool) -> [i32; 4] {
    // 1. Rescale so the largest |component| sits in `[2^29, 2^30)`. A shift
    //    rather than a divide, which costs at most a last bit of the smallest
    //    component — below the resolution of anything downstream.
    let mut largest = v[0].unsigned_abs();
    let mut i = 1;
    while i < 4 {
        if v[i].unsigned_abs() > largest {
            largest = v[i].unsigned_abs();
        }
        i += 1;
    }
    if largest == 0 {
        return [0, 0, 0, ONE as i32];
    }

    let bit_length = corvid_bits::bit_length_u64(largest);
    let t = if bit_length > 30 {
        let down = bit_length - 30;
        [
            shift_down(v[0], down),
            shift_down(v[1], down),
            shift_down(v[2], down),
            shift_down(v[3], down),
        ]
    } else {
        let up = 30 - bit_length;
        [v[0] << up, v[1] << up, v[2] << up, v[3] << up]
    };

    // 2. The sum of squares at Q30. Four squares of at most `2^60` fit `i64`.
    let sum = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2] + t[3] * t[3]) >> 30;

    // 3. Reduce into `[0.25, 1]` by an even shift, then take the reciprocal
    //    square root.
    let (reduced, halve) = if sum > ONE {
        (sum >> 2, true)
    } else {
        (sum, false)
    };
    let inverse = if reduced <= ONE >> 2 {
        // `rsqrt(0.25)` is exactly `2.0`, one step past `I2F30::MAX`, so the
        // type would clamp. That is the axis-aligned case, whose answer ought
        // to be exactly ±1 — so take this one value by hand.
        1i64 << 31
    } else {
        let root = I2F30::from_bits(reduced as i32);
        if fast {
            root.rsqrt_fast()
        } else {
            root.rsqrt()
        }
        .to_bits() as i64
    };

    // 4. Scale each component by it and undo the reduction's shift. The product
    //    of a Q30 component and a Q30 reciprocal reaches `2^61`, so this stays
    //    inside `i64`.
    let shift = if halve { 31 } else { 30 };
    [
        round_shift(t[0] * inverse, shift),
        round_shift(t[1] * inverse, shift),
        round_shift(t[2] * inverse, shift),
        round_shift(t[3] * inverse, shift),
    ]
}

/// `value >> shift`, truncating **toward zero** rather than toward negative
/// infinity.
///
/// An arithmetic shift floors, which makes the rescale asymmetric in sign — and
/// then `normalize(-v)` stops being `-normalize(v)`. That matters here: a
/// rotation and its double-cover twin would pack to bit patterns a last bit
/// apart, and `FineRotation`'s sign canonicalization would not actually
/// canonicalize.
#[inline]
pub(crate) const fn shift_down(value: i64, shift: u32) -> i64 {
    if value >= 0 {
        value >> shift
    } else {
        -((-value) >> shift)
    }
}

/// `round(value / 2^shift)`, half away from zero, as an `i32`.
///
/// The caller's bound guarantees the result lies in `[-2^30, 2^30]`, so the
/// narrowing cannot lose anything.
#[inline]
const fn round_shift(value: i64, shift: u32) -> i32 {
    let half = 1i64 << (shift - 1);
    let rounded = if value >= 0 {
        (value + half) >> shift
    } else {
        -((-value + half) >> shift)
    };
    rounded as i32
}
