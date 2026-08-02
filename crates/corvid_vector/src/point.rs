//! The four 3-vector types.
//!
//! The names read as two independent axes: *Global* means wide range, *Fine*
//! means high resolution. [`GlobalFinePoint`] is both, and pays for it in
//! width; [`GlobalFinePoint`] and [`FinePoint`] share a resolution and differ
//! only in range.
//!
//! Points double as offsets. There is no separate point/vector distinction —
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
/// calculating on the canonical form — the denormal is accepted from
/// `from_bits`, `bytemuck` and `serde` so that raw bits round-trip faithfully,
/// and folded on the way into every operation.
///
/// Everything here reads bit patterns directly, so it has to fold too.
/// Otherwise two [`Direction`]s that compare equal and hash alike would come
/// back with different lengths, dot products and normalized directions — which
/// is exactly the `Hash`/`Eq` disagreement the convention exists to prevent,
/// and it would land in a state hash.
#[inline]
const fn signed32_bits(value: Signed32) -> i32 {
    value.canonicalize().to_bits()
}

/// Generates a 3-vector newtype over one of `corvid_fixed`'s scalars.
///
/// Concrete rather than generic because `const fn` cannot take trait bounds and
/// every operation here is `const`. The parameters carry what the arithmetic
/// needs and the scalar type cannot supply on its own:
///
/// - `wide` holds the exact difference or product of two component bit
///   patterns; `uwide` holds the sum of three squared ones.
/// - `one` is the bit pattern of `1.0`, which is the divisor that takes a
///   product of two components back to component scale. It is a power of two
///   for the fixed-point scalars and `2^31 - 1` for [`Signed32`].
/// - `neg` names the scalar's negation, which is `saturating_neg` for the
///   fixed-point family and `neg` for the symmetric signed-normalized one.
/// - `bits` names the reader that turns a component into its bit pattern. It is
///   not simply `to_bits`, because [`Signed32`] has a redundant encoding — see
///   [`signed32_bits`].
macro_rules! define_point {
    (
        $(#[$attr:meta])*
        $name:ident($scalar:ident) {
            wide: $wide:ty,
            uwide: $uwide:ty,
            one: $one:expr,
            neg: $neg:ident,
            bits: $bits:ident,
        }
    ) => {
        $(#[$attr])*
        #[repr(C)]
        #[derive(Clone, Copy, Default)]
        #[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
        #[cfg_attr(feature = "serde", serde(transparent))]
        #[cfg_attr(feature = "arbitrary", derive(::arbitrary::Arbitrary))]
        #[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
        pub struct $name([$scalar; 3]);

        impl $name {
            /// The origin, and the zero offset.
            pub const ZERO: Self = Self([$scalar::ZERO; 3]);

            /// The bit pattern of `1.0` in the component scalar.
            const ONE_BITS: $wide = $one;

            /// Builds a point from its three components.
            #[must_use]
            #[inline]
            pub const fn new(x: $scalar, y: $scalar, z: $scalar) -> Self {
                Self([x, y, z])
            }

            /// Builds a point with all three components equal.
            #[must_use]
            #[inline]
            pub const fn splat(value: $scalar) -> Self {
                Self([value; 3])
            }

            /// Builds a point from an array, in `x`, `y`, `z` order.
            #[must_use]
            #[inline]
            pub const fn from_array(components: [$scalar; 3]) -> Self {
                Self(components)
            }

            /// The components, in `x`, `y`, `z` order.
            #[must_use]
            #[inline]
            pub const fn to_array(self) -> [$scalar; 3] {
                self.0
            }

            /// The `x` component: rightward, in the crate's right-handed
            /// **+X right, +Y forward, +Z up** convention.
            #[must_use]
            #[inline]
            pub const fn x(self) -> $scalar {
                self.0[0]
            }

            /// The `y` component: forward.
            #[must_use]
            #[inline]
            pub const fn y(self) -> $scalar {
                self.0[1]
            }

            /// The `z` component: upward.
            #[must_use]
            #[inline]
            pub const fn z(self) -> $scalar {
                self.0[2]
            }

            /// Returns `true` if every component is zero.
            #[must_use]
            #[inline]
            pub const fn is_zero(self) -> bool {
                self.0[0].is_zero() && self.0[1].is_zero() && self.0[2].is_zero()
            }

            /// Adds component-wise, clamping each component independently.
            #[must_use]
            #[inline]
            pub const fn add(self, rhs: Self) -> Self {
                Self([
                    self.0[0].saturating_add(rhs.0[0]),
                    self.0[1].saturating_add(rhs.0[1]),
                    self.0[2].saturating_add(rhs.0[2]),
                ])
            }

            /// Adds component-wise, returning `None` if any component overflows.
            #[must_use]
            #[inline]
            pub const fn checked_add(self, rhs: Self) -> Option<Self> {
                match (
                    self.0[0].checked_add(rhs.0[0]),
                    self.0[1].checked_add(rhs.0[1]),
                    self.0[2].checked_add(rhs.0[2]),
                ) {
                    (Some(x), Some(y), Some(z)) => Some(Self([x, y, z])),
                    _ => None,
                }
            }

            /// Subtracts component-wise, clamping each component independently.
            #[must_use]
            #[inline]
            pub const fn sub(self, rhs: Self) -> Self {
                Self([
                    self.0[0].saturating_sub(rhs.0[0]),
                    self.0[1].saturating_sub(rhs.0[1]),
                    self.0[2].saturating_sub(rhs.0[2]),
                ])
            }

            /// Subtracts component-wise, returning `None` if any component
            /// overflows.
            ///
            /// This is the honest way to take an offset between two points that
            /// might be further apart than the type can express — which is
            /// exactly why `corvid_transform` widens before it subtracts.
            #[must_use]
            #[inline]
            pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
                match (
                    self.0[0].checked_sub(rhs.0[0]),
                    self.0[1].checked_sub(rhs.0[1]),
                    self.0[2].checked_sub(rhs.0[2]),
                ) {
                    (Some(x), Some(y), Some(z)) => Some(Self([x, y, z])),
                    _ => None,
                }
            }

            /// Scales by a scalar, clamping each component independently.
            #[must_use]
            #[inline]
            pub const fn mul(self, rhs: $scalar) -> Self {
                Self([
                    self.0[0].saturating_mul(rhs),
                    self.0[1].saturating_mul(rhs),
                    self.0[2].saturating_mul(rhs),
                ])
            }

            /// Scales by a scalar, returning `None` if any component overflows.
            #[must_use]
            #[inline]
            pub const fn checked_mul(self, rhs: $scalar) -> Option<Self> {
                match (
                    self.0[0].checked_mul(rhs),
                    self.0[1].checked_mul(rhs),
                    self.0[2].checked_mul(rhs),
                ) {
                    (Some(x), Some(y), Some(z)) => Some(Self([x, y, z])),
                    _ => None,
                }
            }

            /// Negates every component.
            #[must_use]
            #[inline]
            pub const fn neg(self) -> Self {
                Self([self.0[0].$neg(), self.0[1].$neg(), self.0[2].$neg()])
            }

            /// The component-wise absolute value.
            #[must_use]
            #[inline]
            pub const fn abs(self) -> Self {
                Self([self.0[0].abs(), self.0[1].abs(), self.0[2].abs()])
            }

            /// The component-wise minimum.
            #[must_use]
            #[inline]
            pub const fn min(self, rhs: Self) -> Self {
                Self([
                    self.0[0].min(rhs.0[0]),
                    self.0[1].min(rhs.0[1]),
                    self.0[2].min(rhs.0[2]),
                ])
            }

            /// The component-wise maximum.
            #[must_use]
            #[inline]
            pub const fn max(self, rhs: Self) -> Self {
                Self([
                    self.0[0].max(rhs.0[0]),
                    self.0[1].max(rhs.0[1]),
                    self.0[2].max(rhs.0[2]),
                ])
            }

            /// Clamps component-wise into the box `min ..= max`.
            ///
            /// Cannot panic: where `min` exceeds `max` on an axis, the bound
            /// applied last wins, inherited from the scalar's own `clamp`.
            #[must_use]
            #[inline]
            pub const fn clamp(self, min: Self, max: Self) -> Self {
                Self([
                    self.0[0].clamp(min.0[0], max.0[0]),
                    self.0[1].clamp(min.0[1], max.0[1]),
                    self.0[2].clamp(min.0[2], max.0[2]),
                ])
            }

            /// Linearly interpolates toward `to`, component-wise.
            ///
            /// Exact at both ends, inherited from the scalar's own `lerp`.
            #[must_use]
            #[inline]
            pub const fn lerp(self, to: Self, weight: Factor32) -> Self {
                Self([
                    self.0[0].lerp(to.0[0], weight),
                    self.0[1].lerp(to.0[1], weight),
                    self.0[2].lerp(to.0[2], weight),
                ])
            }

            /// The dot product, in units of `DELTA²`.
            ///
            /// Returns the widened intermediate rather than the component
            /// scalar, for the same reason
            /// [`length_squared`](Self::length_squared) does: expressing it
            /// back in the point's own type would saturate for perfectly
            /// ordinary vectors.
            ///
            /// Exact for the 32-bit component types. For
            /// [`GlobalFinePoint`], whose components are `i64`, the sum of
            /// three products can pass `i128`'s range at the far corners of
            /// the world, where it saturates.
            #[must_use]
            #[inline]
            pub const fn dot(self, rhs: Self) -> i128 {
                let x = ($bits(self.0[0]) as i128) * ($bits(rhs.0[0]) as i128);
                let y = ($bits(self.0[1]) as i128) * ($bits(rhs.0[1]) as i128);
                let z = ($bits(self.0[2]) as i128) * ($bits(rhs.0[2]) as i128);
                x.saturating_add(y).saturating_add(z)
            }

            /// The cross product, clamping each component independently.
            ///
            /// Right-handed: `X × Y = Z`, which is what makes
            /// `right = forward × up` come out consistent.
            #[must_use]
            #[inline]
            pub const fn cross(self, rhs: Self) -> Self {
                Self([
                    Self::descale(Self::cross_term(self.0[1], rhs.0[2], self.0[2], rhs.0[1])),
                    Self::descale(Self::cross_term(self.0[2], rhs.0[0], self.0[0], rhs.0[2])),
                    Self::descale(Self::cross_term(self.0[0], rhs.0[1], self.0[1], rhs.0[0])),
                ])
            }

            /// `a·b − c·d` at full width, saturating.
            ///
            /// The two products each fit `wide`; their difference can want one
            /// bit more, and only for operands at the very corners of the type,
            /// where the result is far outside the point's range anyway.
            #[inline]
            const fn cross_term(a: $scalar, b: $scalar, c: $scalar, d: $scalar) -> $wide {
                let left = ($bits(a) as $wide) * ($bits(b) as $wide);
                let right = ($bits(c) as $wide) * ($bits(d) as $wide);
                left.saturating_sub(right)
            }

            /// Divides a product back to component scale, rounded and saturated.
            #[inline]
            const fn descale(product: $wide) -> $scalar {
                let quotient = product / Self::ONE_BITS;
                let remainder = product % Self::ONE_BITS;
                // `|remainder| < ONE_BITS`, so doubling it cannot overflow.
                let rounded = if remainder * 2 >= Self::ONE_BITS {
                    quotient + 1
                } else if remainder * 2 <= -Self::ONE_BITS {
                    quotient - 1
                } else {
                    quotient
                };
                if rounded > $scalar::MAX.to_bits() as $wide {
                    $scalar::MAX
                } else if rounded < $scalar::MIN.to_bits() as $wide {
                    $scalar::MIN
                } else {
                    $scalar::from_bits(rounded as _)
                }
            }

            /// The squared length, in units of `DELTA²`.
            ///
            /// **This deliberately does not return the point's own scalar
            /// type.** `GlobalPoint`'s components reach ±8388608, so the raw
            /// sum of three squares reaches `3 × 2^62` — past `i64::MAX` — and
            /// expressing the result back in `I24F8` would saturate for any
            /// vector longer than 1672 m. A `length_squared` that silently
            /// returns `MAX` for a 2 km offset is worse than no
            /// `length_squared` at all.
            ///
            /// The widened *unsigned* intermediate is lossless over the whole
            /// range and answers the question the operation is actually for:
            /// comparing and sorting distances without a square root.
            #[must_use]
            #[inline]
            pub const fn length_squared(self) -> $uwide {
                let x = $bits(self.0[0]).unsigned_abs() as $uwide;
                let y = $bits(self.0[1]).unsigned_abs() as $uwide;
                let z = $bits(self.0[2]).unsigned_abs() as $uwide;
                x * x + y * y + z * z
            }

            /// The length, rounded to the component type's resolution.
            ///
            /// Computed the way [`corvid_fixed`]'s `hypot` already does it:
            /// unsigned wide sum of squares, one integer square root, one
            /// rounding. Because a value is its bit pattern over a fixed scale,
            /// the integer square root of the summed squares *is* the result's
            /// bit pattern — there is no rescaling step to lose anything in.
            ///
            /// Saturates at the component type's `MAX`, which a
            /// [`GlobalFinePoint`] reaches at the far corners of the world:
            /// `√3 × 1.407e14` exceeds `I48F16`'s own range.
            #[must_use]
            #[inline]
            pub const fn length(self) -> $scalar {
                let squared = self.length_squared();
                let root = squared.isqrt();
                // Round up when the true root is past the halfway point, which
                // happens exactly when the remainder exceeds the root.
                let rounded = if squared - root * root > root { root + 1 } else { root };
                if rounded > $scalar::MAX.to_bits() as $uwide {
                    $scalar::MAX
                } else {
                    $scalar::from_bits(rounded as _)
                }
            }

            /// The distance to another point.
            ///
            /// Saturates at the component type's `MAX` when the points are further
            /// apart than the type can express — which for a
            /// [`GlobalFinePoint`] includes opposite corners of the world, and
            /// is documented rather than hidden.
            #[must_use]
            #[inline]
            pub const fn distance(self, other: Self) -> $scalar {
                match self.checked_sub(other) {
                    // A difference that does not fit the type is certainly
                    // longer than the type can express.
                    None => $scalar::MAX,
                    Some(offset) => offset.length(),
                }
            }

            /// The unit direction along this vector, or `None` if it is zero.
            ///
            /// A zero vector has no direction, which is the only failure.
            ///
            /// Only the ratios of the components matter, so this normalizes
            /// the raw bit patterns and never touches the component scale.
            /// Rescaling is a shift rather than a divide, so the same direction
            /// at two magnitudes can differ in the last bit or two — the result
            /// is deterministic, not magnitude-independent to the bit. One
            /// [`rsqrt`](corvid_fixed::I2F30::rsqrt), three multiplies, and a
            /// handful of shifts — no division anywhere.
            #[must_use]
            #[inline]
            pub const fn normalize(self) -> Option<Direction> {
                let bits = [
                    $bits(self.0[0]) as i128,
                    $bits(self.0[1]) as i128,
                    $bits(self.0[2]) as i128,
                ];
                crate::point::normalize_bits(bits, false)
            }

            /// The unit direction along this vector, approximately, or `None`
            /// if it is zero.
            ///
            /// [`normalize`](Self::normalize) over
            /// [`rsqrt_fast`](corvid_fixed::I2F30::rsqrt_fast) rather than
            /// [`rsqrt`](corvid_fixed::I2F30::rsqrt) — about 3.7x the
            /// throughput of that step, for a direction good to `3.2e-5`
            /// rather than to [`Direction`]'s own last bit.
            ///
            /// Relative to the angles a renderer resolves that is about
            /// 0.002°, so this is the tier for a look-at or a face-normal
            /// recomputed per frame. It is the wrong one for an axis a
            /// rotation will be built from and then composed repeatedly, where
            /// the error compounds instead of being consumed.
            #[must_use]
            #[inline]
            pub const fn normalize_fast(self) -> Option<Direction> {
                let bits = [
                    $bits(self.0[0]) as i128,
                    $bits(self.0[1]) as i128,
                    $bits(self.0[2]) as i128,
                ];
                crate::point::normalize_bits(bits, true)
            }

            /// The unit direction from here to `other`, or `None` if the two
            /// coincide.
            ///
            /// The difference is taken at full width, **not** through
            /// [`sub`](Self::sub): the saturating difference clamps each axis
            /// independently, which does not preserve a bearing — two points
            /// past the type's range in `x` and half that in `y` would come
            /// back as a 45° heading. Widening first only helps a type that
            /// has headroom left, which the widest one does not, so the
            /// subtraction happens here instead.
            #[must_use]
            #[inline]
            pub const fn direction_to(self, other: Self) -> Option<Direction> {
                let bits = [
                    $bits(other.0[0]) as i128 - $bits(self.0[0]) as i128,
                    $bits(other.0[1]) as i128 - $bits(self.0[1]) as i128,
                    $bits(other.0[2]) as i128 - $bits(self.0[2]) as i128,
                ];
                crate::point::normalize_bits(bits, false)
            }

            /// The unit direction from here to `other`, approximately, or
            /// `None` if the two coincide.
            ///
            /// [`direction_to`](Self::direction_to) over the approximate
            /// reciprocal square root; see
            /// [`normalize_fast`](Self::normalize_fast) for what that trades.
            /// The subtraction is still exact at full width — only the
            /// normalize is approximate.
            #[must_use]
            #[inline]
            pub const fn direction_to_fast(self, other: Self) -> Option<Direction> {
                let bits = [
                    $bits(other.0[0]) as i128 - $bits(self.0[0]) as i128,
                    $bits(other.0[1]) as i128 - $bits(self.0[1]) as i128,
                    $bits(other.0[2]) as i128 - $bits(self.0[2]) as i128,
                ];
                crate::point::normalize_bits(bits, true)
            }
        }

        impl PartialEq for $name {
            #[inline]
            fn eq(&self, other: &Self) -> bool {
                self.0[0] == other.0[0] && self.0[1] == other.0[1] && self.0[2] == other.0[2]
            }
        }

        impl Eq for $name {}

        impl core::hash::Hash for $name {
            #[inline]
            fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
                self.0[0].hash(state);
                self.0[1].hash(state);
                self.0[2].hash(state);
            }
        }

        impl core::fmt::Debug for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(
                    f,
                    concat!(stringify!($name), "({}, {}, {})"),
                    self.0[0].to_f64(),
                    self.0[1].to_f64(),
                    self.0[2].to_f64()
                )
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(
                    f,
                    "({}, {}, {})",
                    self.0[0].to_f64(),
                    self.0[1].to_f64(),
                    self.0[2].to_f64()
                )
            }
        }

        impl core::ops::Add for $name {
            type Output = Self;

            #[inline]
            fn add(self, rhs: Self) -> Self {
                Self::add(self, rhs)
            }
        }

        impl core::ops::AddAssign for $name {
            #[inline]
            fn add_assign(&mut self, rhs: Self) {
                *self = Self::add(*self, rhs);
            }
        }

        impl core::ops::Sub for $name {
            type Output = Self;

            #[inline]
            fn sub(self, rhs: Self) -> Self {
                Self::sub(self, rhs)
            }
        }

        impl core::ops::SubAssign for $name {
            #[inline]
            fn sub_assign(&mut self, rhs: Self) {
                *self = Self::sub(*self, rhs);
            }
        }

        impl core::ops::Mul<$scalar> for $name {
            type Output = Self;

            #[inline]
            fn mul(self, rhs: $scalar) -> Self {
                Self::mul(self, rhs)
            }
        }

        impl core::ops::MulAssign<$scalar> for $name {
            #[inline]
            fn mul_assign(&mut self, rhs: $scalar) {
                *self = Self::mul(*self, rhs);
            }
        }

        impl core::ops::Neg for $name {
            type Output = Self;

            #[inline]
            fn neg(self) -> Self {
                Self::neg(self)
            }
        }

        impl From<[$scalar; 3]> for $name {
            #[inline]
            fn from(components: [$scalar; 3]) -> Self {
                Self(components)
            }
        }

        impl From<$name> for [$scalar; 3] {
            #[inline]
            fn from(point: $name) -> Self {
                point.0
            }
        }
    };
}

define_point! {
    /// A world-space position at both wide range and high resolution.
    ///
    /// | | |
    /// |---|---|
    /// | Component | [`I48F16`] |
    /// | Range | ±1.407e14 m |
    /// | Resolution | 15.26 µm |
    ///
    /// The camera's and the VR tracked poses' position type, and the width
    /// every transform widens into before it subtracts.
    GlobalFinePoint(I48F16) {
        wide: i128,
        uwide: u128,
        one: 65_536,
        neg: saturating_neg,
        bits: i48f16_bits,
    }
}

define_point! {
    /// A world-space position for ordinary objects, and the everyday offset.
    ///
    /// | | |
    /// |---|---|
    /// | Component | [`I24F8`] |
    /// | Range | ±8388 km |
    /// | Resolution | 3.9 mm |
    GlobalPoint(I24F8) {
        wide: i64,
        uwide: u64,
        one: 256,
        neg: saturating_neg,
        bits: i24f8_bits,
    }
}

define_point! {
    /// A near-field offset, for what the renderer and the eye actually see.
    ///
    /// | | |
    /// |---|---|
    /// | Component | [`I16F16`] |
    /// | Range | ±32.7 km |
    /// | Resolution | 15.26 µm |
    ///
    /// Shares its 16 fractional bits with [`GlobalFinePoint`], so narrowing a
    /// difference into this type is a range check and nothing else.
    FinePoint(I16F16) {
        wide: i64,
        uwide: u64,
        one: 65_536,
        neg: saturating_neg,
        bits: i16f16_bits,
    }
}

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
    /// it is a boundary type, and direction maths is cold — once per object
    /// per frame at most, against thousands of point rotations.
    Direction(Signed32) {
        wide: i64,
        uwide: u64,
        one: 2_147_483_647,
        neg: neg,
        bits: signed32_bits,
    }
}

/// Normalizes three raw bit patterns into a unit [`Direction`].
///
/// Shared by all four point types, because normalizing is scale-free: the
/// component scale cancels, so only the ratios matter and one implementation
/// serves every input width.
///
/// The reduction in step 3 is the reason [`I2F30`] exists in the shape it does.
/// The sum of squares, relative to the largest component, lands in `[0.25, 3]`,
/// which does not fit a type that stops at `±2` — but the sum can always be
/// brought into `[0.25, 1)` by an *even* shift, and `rsqrt` of that lands in
/// `(1, 2]`, which fits exactly.
///
/// `fast` picks the approximate reciprocal square root in step 3 and changes
/// nothing else. It is a literal at every call site, so the branch folds away
/// and neither tier pays for the other's existence.
#[inline]
const fn normalize_bits(bits: [i128; 3], fast: bool) -> Option<Direction> {
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
        // ought to be exactly ±1 — so take this one value by hand rather than
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
    //    `Signed32` — all in one expression, so the whole normalize rounds
    //    exactly once. `Signed32`'s scale is `2^31 − 1` rather than a power of
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
/// An arithmetic shift floors, which makes the rescale asymmetric in sign — and
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

/// `round(t · inverse · (2^31 − 1) / 2^(shift + 30))`, clamped into
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
