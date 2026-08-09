//! The generator for the four point types: the newtype, and the arithmetic
//! that works a component at a time.
//!
//! What a point is made of and what it does elementwise live here; the
//! geometry -- which mixes the three components together -- is in
//! [`define_point_geometry`](super::geometry::define_point_geometry), and the
//! operator and formatting impls are in
//! [`define_point_traits`](super::traits::define_point_traits). All three
//! expand into the same type, so a method one declares is reachable from
//! another.

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
///   not simply `to_bits`, because [`Signed32`] has a redundant encoding -- see
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
            /// might be further apart than the type can express -- which is
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
        }
    };
}

pub(super) use define_point;
