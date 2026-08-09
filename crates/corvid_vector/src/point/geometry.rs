//! The generator for the geometry: the operations that mix a point's three
//! components together.
//!
//! Split from [`define_point`](super::base::define_point) because a file stays
//! under 400 lines, and this is the seam that was already there: everything
//! that generator declares works a component at a time, and nothing here does.

macro_rules! define_point_geometry {
    ($name:ident, $scalar:ident, $wide:ty, $uwide:ty, $bits:ident) => {
        impl $name {
            /// The dot product, in units of `DELTA^2`.
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
            /// Right-handed: `X x Y = Z`, which is what makes
            /// `right = forward x up` come out consistent.
            #[must_use]
            #[inline]
            pub const fn cross(self, rhs: Self) -> Self {
                Self([
                    Self::descale(Self::cross_term(self.0[1], rhs.0[2], self.0[2], rhs.0[1])),
                    Self::descale(Self::cross_term(self.0[2], rhs.0[0], self.0[0], rhs.0[2])),
                    Self::descale(Self::cross_term(self.0[0], rhs.0[1], self.0[1], rhs.0[0])),
                ])
            }

            /// `a*b - c*d` at full width, saturating.
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

            /// The squared length, in units of `DELTA^2`.
            ///
            /// **This deliberately does not return the point's own scalar
            /// type.** `GlobalPoint`'s components reach +/-8388608, so the raw
            /// sum of three squares reaches `3 x 2^62` -- past `i64::MAX` -- and
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
            /// bit pattern -- there is no rescaling step to lose anything in.
            ///
            /// Saturates at the component type's `MAX`, which a
            /// [`GlobalFinePoint`] reaches at the far corners of the world:
            /// `sqrt(3) x 1.407e14` exceeds `I48F16`'s own range.
            #[must_use]
            #[inline]
            pub const fn length(self) -> $scalar {
                let squared = self.length_squared();
                let root = squared.isqrt();
                // Round up when the true root is past the halfway point, which
                // happens exactly when the remainder exceeds the root.
                let rounded = if squared - root * root > root {
                    root + 1
                } else {
                    root
                };
                if rounded > $scalar::MAX.to_bits() as $uwide {
                    $scalar::MAX
                } else {
                    $scalar::from_bits(rounded as _)
                }
            }

            /// The distance to another point.
            ///
            /// Saturates at the component type's `MAX` when the points are further
            /// apart than the type can express -- which for a
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
            /// at two magnitudes can differ in the last bit or two -- the result
            /// is deterministic, not magnitude-independent to the bit. One
            /// [`rsqrt`](corvid_fixed::I2F30::rsqrt), three multiplies, and a
            /// handful of shifts -- no division anywhere.
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
            /// [`rsqrt`](corvid_fixed::I2F30::rsqrt) -- about 3.7x the
            /// throughput of that step, for a direction good to `3.2e-5`
            /// rather than to [`Direction`]'s own last bit.
            ///
            /// Relative to the angles a renderer resolves that is about
            /// 0.002 deg, so this is the tier for a look-at or a face-normal
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
            /// independently, which does not preserve a bearing -- two points
            /// past the type's range in `x` and half that in `y` would come
            /// back as a 45 deg heading. Widening first only helps a type that
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
            /// The subtraction is still exact at full width -- only the
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
    };
}

pub(super) use define_point_geometry;
