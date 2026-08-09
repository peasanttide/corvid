//! The four 3-vector types.
//!
//! The names read as two independent axes: *Global* means wide range, *Fine*
//! means high resolution. [`GlobalFinePoint`] is both, and pays for it in
//! width; [`GlobalFinePoint`] and [`FinePoint`] share a resolution and differ
//! only in range.
//!
//! Points double as offsets. There is no separate point/vector distinction --
//! the same choice Godot and Unity make, and a separate `GlobalPointOffset`
//! would double the API for no caught bug.

use corvid_fixed::{Factor32, I2F30, I16F16, I24F8, I48F16, Signed32};

/// A component's bit pattern.
///
/// The three fixed-point scalars encode each value exactly once, so reading one
/// is [`to_bits`](I48F16::to_bits) and nothing more. [`signed32_bits`] is the
/// one that has work to do.
#[inline]
const fn i48f16_bits(value: I48F16) -> i64 {
    value.to_bits()
}

/// A component's bit pattern. See [`i48f16_bits`].
#[inline]
const fn i24f8_bits(value: I24F8) -> i32 {
    value.to_bits()
}

/// A component's bit pattern. See [`i48f16_bits`].
#[inline]
const fn i16f16_bits(value: I16F16) -> i32 {
    value.to_bits()
}

/// A [`Signed32`] component's **canonical** bit pattern.
///
/// `SNORM` spends one pattern twice: `i32::MIN` and `-(2^31 - 1)` both denote
/// `-1.0`, and `corvid_fixed` resolves that by comparing, hashing and
/// calculating on the canonical form -- the denormal is accepted from
/// `from_bits`, `bytemuck` and `serde` so that raw bits round-trip faithfully,
/// and folded on the way into every operation.
///
/// Everything here reads bit patterns directly, so it has to fold too.
/// Otherwise two [`Direction`]s that compare equal and hash alike would come
/// back with different lengths, dot products and normalized directions -- which
/// is exactly the `Hash`/`Eq` disagreement the convention exists to prevent,
/// and it would land in a state hash.
#[inline]
const fn signed32_bits(value: Signed32) -> i32 {
    value.canonicalize().to_bits()
}
mod base;
mod geometry;
mod traits;

use base::define_point;
use geometry::define_point_geometry;
use traits::define_point_traits;

define_point! {
    /// A world-space position at both wide range and high resolution.
    ///
    /// | | |
    /// |---|---|
    /// | Component | [`I48F16`] |
    /// | Range | +/-1.407e14 m |
    /// | Resolution | 15.26 um |
    ///
    /// The camera's and the VR tracked poses' position type, and the width
    /// every transform widens into before it subtracts.
    GlobalFinePoint(I48F16) {
        wide: i128,
        uwide: u128,
        one: 65_536,
        neg: saturating_neg,
        bits: i48f16_bits,
        build: globalfinepoint,
    }
}

define_point_geometry!(GlobalFinePoint, I48F16, i128, u128, i48f16_bits);
define_point_traits!(GlobalFinePoint, I48F16);

define_point! {
    /// A world-space position for ordinary objects, and the everyday offset.
    ///
    /// | | |
    /// |---|---|
    /// | Component | [`I24F8`] |
    /// | Range | +/-8388 km |
    /// | Resolution | 3.9 mm |
    GlobalPoint(I24F8) {
        wide: i64,
        uwide: u64,
        one: 256,
        neg: saturating_neg,
        bits: i24f8_bits,
        build: globalpoint,
    }
}

define_point_geometry!(GlobalPoint, I24F8, i64, u64, i24f8_bits);
define_point_traits!(GlobalPoint, I24F8);

define_point! {
    /// A near-field offset, for what the renderer and the eye actually see.
    ///
    /// | | |
    /// |---|---|
    /// | Component | [`I16F16`] |
    /// | Range | +/-32.7 km |
    /// | Resolution | 15.26 um |
    ///
    /// Shares its 16 fractional bits with [`GlobalFinePoint`], so narrowing a
    /// difference into this type is a range check and nothing else.
    FinePoint(I16F16) {
        wide: i64,
        uwide: u64,
        one: 65_536,
        neg: saturating_neg,
        bits: i16f16_bits,
        build: finepoint,
    }
}

define_point_geometry!(FinePoint, I16F16, i64, u64, i16f16_bits);
define_point_traits!(FinePoint, I16F16);

define_point! {
    /// A unit direction, or a rotation axis.
    ///
    /// | | |
    /// |---|---|
    /// | Component | [`Signed32`] |
    /// | Range | unit |
    /// | Resolution | 4.7e-10 |
    ///
    /// The bit patterns match `wgpu`'s `Snorm32`, which is why this stays
    /// [`Signed32`] rather than moving to `I2F30` with the rotation matrices:
    /// it is a boundary type, and direction maths is cold -- once per object
    /// per frame at most, against thousands of point rotations.
    Direction(Signed32) {
        wide: i64,
        uwide: u64,
        one: 2_147_483_647,
        neg: neg,
        bits: signed32_bits,
        build: direction,
    }
}

define_point_geometry!(Direction, Signed32, i64, u64, signed32_bits);
define_point_traits!(Direction, Signed32);

impl Direction {
    /// Right, in the workspace's right-handed +X right, +Y forward, +Z up
    /// convention.
    ///
    /// Written as a constant rather than reached through
    /// [`normalize`](Direction::normalize), which answers an [`Option`]
    /// because it has to: an axis is known to be a unit vector, and a caller
    /// naming one should not be handed a `None` arm it can never take.
    pub const X: Self = Self([Signed32::MAX, Signed32::ZERO, Signed32::ZERO]);

    /// Forward.
    pub const Y: Self = Self([Signed32::ZERO, Signed32::MAX, Signed32::ZERO]);

    /// Up.
    pub const Z: Self = Self([Signed32::ZERO, Signed32::ZERO, Signed32::MAX]);
}

/// Normalizes three raw bit patterns into a unit [`Direction`].
///
/// Shared by all four point types, because normalizing is scale-free: the
/// component scale cancels, so only the ratios matter and one implementation
/// serves every input width.
///
/// The reduction in step 3 is the reason [`I2F30`] exists in the shape it does.
/// The sum of squares, relative to the largest component, lands in `[0.25, 3]`,
/// which does not fit a type that stops at `+/-2` -- but the sum can always be
/// brought into `[0.25, 1)` by an *even* shift, and `rsqrt` of that lands in
/// `(1, 2]`, which fits exactly.
///
/// `fast` picks the approximate reciprocal square root in step 3 and changes
/// nothing else. It is a literal at every call site, so the branch folds away
/// and neither tier pays for the other's existence.
#[inline]
pub(crate) const fn normalize_bits(bits: [i128; 3], fast: bool) -> Option<Direction> {
    // 1. Rescale so the largest component sits just under 2^30. Shifting rather
    //    than dividing costs at most a last bit of the smallest component,
    //    which is below a unit vector's own resolution.
    let mut largest = bits[0].unsigned_abs();
    if bits[1].unsigned_abs() > largest {
        largest = bits[1].unsigned_abs();
    }
    if bits[2].unsigned_abs() > largest {
        largest = bits[2].unsigned_abs();
    }
    if largest == 0 {
        // A zero vector has no direction to normalize. The only failure.
        return None;
    }

    let bit_length = 128 - largest.leading_zeros();
    let scaled = if bit_length > 30 {
        let down = bit_length - 30;
        [
            shift_down(bits[0], down),
            shift_down(bits[1], down),
            shift_down(bits[2], down),
        ]
    } else {
        let up = 30 - bit_length;
        [bits[0] << up, bits[1] << up, bits[2] << up]
    };

    // Every component now fits comfortably inside `i64`, and the largest is in
    // `[2^29, 2^30)`.
    let t = [scaled[0] as i64, scaled[1] as i64, scaled[2] as i64];

    // 2. The sum of squares at Q30. Each square is at most 2^60 and there are
    //    three of them, so this stays inside `i64`.
    let sum = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]) >> 30;

    // 3. Bring it into `[0.25, 1)` by an even shift, so `rsqrt` lands in the
    //    `(1, 2]` that `I2F30` was given its spare bit of range for.
    let (reduced, halve) = if sum >= 1 << 30 {
        (sum >> 2, true)
    } else {
        (sum, false)
    };
    let inverse = if reduced == 1 << 28 {
        // `rsqrt(0.25)` is exactly `2.0`, one step past `I2F30::MAX`, so the
        // type saturates. That is precisely the axis-aligned case, whose answer
        // ought to be exactly +/-1 -- so take this one value by hand rather than
        // lose a last bit to a clamp.
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

    // 4. Scale each component by it, undo the reduction's shift, and convert to
    //    `Signed32` -- all in one expression, so the whole normalize rounds
    //    exactly once. `Signed32`'s scale is `2^31 - 1` rather than a power of
    //    two, which is the only non-shift factor in the whole routine.
    let shift = if halve { 31 } else { 30 };
    Some(Direction::new(
        unit_component(t[0], inverse, shift),
        unit_component(t[1], inverse, shift),
        unit_component(t[2], inverse, shift),
    ))
}

/// `value >> shift`, truncating **toward zero** rather than toward negative
/// infinity.
///
/// An arithmetic shift floors, which makes the rescale asymmetric in sign -- and
/// then `normalize(-v)` stops being `-normalize(v)`. A direction and its
/// opposite would come back a last bit apart instead of exact negatives, so
/// mirroring a scene, or reversing a ray, would not reproduce.
#[inline]
const fn shift_down(value: i128, shift: u32) -> i128 {
    if value >= 0 {
        value >> shift
    } else {
        -((-value) >> shift)
    }
}

/// `round(t * inverse * (2^31 - 1) / 2^(shift + 30))`, clamped into
/// [`Signed32`].
///
/// The three factors reach `2^30`, `2^31` and `2^31`, so the product needs
/// `i128`; the divisor is a shift. One rounding, from the full-width product.
#[inline]
const fn unit_component(t: i64, inverse: i64, shift: u32) -> Signed32 {
    let scale = Signed32::MAX.to_bits() as i128;
    let scaled = (t as i128) * (inverse as i128) * scale;
    let total = shift + 30;
    let half = 1i128 << (total - 1);
    let rounded = if scaled >= 0 {
        (scaled + half) >> total
    } else {
        -((-scaled + half) >> total)
    };

    let limit = Signed32::MAX.to_bits() as i128;
    if rounded > limit {
        Signed32::MAX
    } else if rounded < -limit {
        Signed32::MIN
    } else {
        Signed32::from_bits(rounded as i32)
    }
}
