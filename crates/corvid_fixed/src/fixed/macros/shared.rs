//! The generators every family uses: the newtype itself, the inherent surface
//! shared across all five, and the operator impls that stand for it.

/// Declares a family newtype, applying the optional interop derives.
///
/// Comparison, hashing, and formatting are deliberately not derived:
/// [`impl_shared`] routes them through `cmp_key`, so that the signed-normalized
/// family's two encodings of `-1.0` -- and a pitch's bit patterns from outside
/// `MIN ..= MAX` -- behave as the one value they denote.
macro_rules! define_newtype {
    (
        $(#[$attr:meta])*
        $name:ident($repr:ty)
    ) => {
        $(#[$attr])*
        #[repr(transparent)]
        #[derive(Clone, Copy, Default)]
        #[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
        #[cfg_attr(feature = "serde", serde(transparent))]
        #[cfg_attr(feature = "arbitrary", derive(::arbitrary::Arbitrary))]
        #[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
        pub struct $name($repr);
    };
}

/// Implements bit access, comparison, and formatting.
macro_rules! impl_shared {
    ($name:ident, $repr:ty, $unit:expr) => {
        impl $name {
            /// The zero value.
            pub const ZERO: Self = Self(0);

            /// Number of bits of storage.
            pub const BITS: u32 = <$repr>::BITS;

            #[doc = concat!("Wraps a raw bit pattern as a `", stringify!($name), "`.")]
            ///
            /// Every bit pattern is accepted, so this is the exact inverse of
            /// [`to_bits`](Self::to_bits) and is what `bytemuck` and `serde`
            /// round-trip through.
            #[must_use]
            #[inline]
            pub const fn from_bits(bits: $repr) -> Self {
                Self(bits)
            }

            /// The raw bit pattern.
            #[must_use]
            #[inline]
            pub const fn to_bits(self) -> $repr {
                self.0
            }

            /// Returns the lesser of two values.
            ///
            /// The result is canonical. Where `cmp_key` folds -- the
            /// signed-normalized denormal, a pitch's out-of-range bits -- the
            /// folded pattern is what comes back, so a bit pattern the type does
            /// not mean cannot leave through `min`, `max`, or `clamp`.
            /// [`Ord`]'s same-named methods forward here rather than using their
            /// defaults, so generic `T: Ord` code gets the same guarantee.
            /// Picking a value out some other way -- [`Iterator::max`], a sort --
            /// hands back the element as it was given, bits and all.
            #[must_use]
            #[inline]
            pub const fn min(self, other: Self) -> Self {
                let (this, that) = (self.cmp_key(), other.cmp_key());
                Self(if this <= that { this } else { that })
            }

            /// Returns the greater of two values.
            ///
            /// Canonical, like [`min`](Self::min).
            #[must_use]
            #[inline]
            pub const fn max(self, other: Self) -> Self {
                let (this, that) = (self.cmp_key(), other.cmp_key());
                Self(if this >= that { this } else { that })
            }

            /// Clamps to the inclusive range `min ..= max`.
            ///
            /// Unlike [`Ord::clamp`]'s default this cannot panic: when
            /// `min > max` the bound applied last wins and `max` is returned.
            /// [`Ord::clamp`] forwards here, so it cannot panic on this type
            /// either.
            ///
            /// The result is canonical, inherited from [`min`](Self::min) and
            /// [`max`](Self::max), so the bit pattern that comes back really does
            /// lie in `min ..= max` and not merely equal something that does.
            #[must_use]
            #[inline]
            pub const fn clamp(self, min: Self, max: Self) -> Self {
                self.max(min).min(max)
            }

            /// Returns `true` if this is zero.
            #[must_use]
            #[inline]
            pub const fn is_zero(self) -> bool {
                self.cmp_key() == 0
            }

            /// The value as an `f32`.
            ///
            /// Correctly rounded for every type whose
            /// [`to_f64`](Self::to_f64) is exact, which is every one but
            /// [`I48F16`](crate::I48F16): there the `f64` intermediate is
            /// itself a rounding, so a value sitting beside an `f32` halfway
            /// point can round twice and land one `f32` step out.
            #[must_use]
            #[inline]
            pub const fn to_f32(self) -> f32 {
                self.to_f64() as f32
            }

            #[doc = concat!("Converts from `f32`, saturating at the bounds of `", stringify!($name), "`.")]
            ///
            /// See [`from_f64`](Self::from_f64) for the rounding and
            /// non-finite behavior, which this shares.
            #[must_use]
            #[inline]
            pub const fn from_f32(value: f32) -> Self {
                Self::from_f64(value as f64)
            }

            /// Converts from `f32`, or returns `None` if out of range.
            ///
            /// `NaN` returns `None`.
            #[must_use]
            #[inline]
            pub const fn checked_from_f32(value: f32) -> Option<Self> {
                Self::checked_from_f64(value as f64)
            }
        }

        impl PartialEq for $name {
            #[inline]
            fn eq(&self, other: &Self) -> bool {
                self.cmp_key() == other.cmp_key()
            }
        }

        impl Eq for $name {}

        impl PartialOrd for $name {
            #[inline]
            fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for $name {
            #[inline]
            fn cmp(&self, other: &Self) -> core::cmp::Ordering {
                self.cmp_key().cmp(&other.cmp_key())
            }

            // The provided methods would return the winning operand untouched,
            // dropping the canonicalization the inherent versions guarantee, and
            // the default `clamp` asserts `min <= max`. Forwarding all three
            // keeps generic `T: Ord` code behaving like the concrete type. Ties
            // are unobservable either way: two values that compare equal have
            // the same `cmp_key`, so they leave here with the same bits.
            #[inline]
            fn min(self, other: Self) -> Self {
                Self::min(self, other)
            }

            #[inline]
            fn max(self, other: Self) -> Self {
                Self::max(self, other)
            }

            #[inline]
            fn clamp(self, min: Self, max: Self) -> Self {
                Self::clamp(self, min, max)
            }
        }

        impl core::hash::Hash for $name {
            #[inline]
            fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
                self.cmp_key().hash(state);
            }
        }

        impl core::fmt::Display for $name {
            /// Formats the value, honoring width and precision specifiers.
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                core::fmt::Display::fmt(&self.to_f64(), f)
            }
        }

        impl core::fmt::Debug for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, concat!(stringify!($name), "({}", $unit, ")"), self.to_f64())
            }
        }
    };
}

/// Implements a binary operator and its compound-assignment form.
macro_rules! impl_binop {
    ($name:ident, $op:ident::$method:ident, $assign:ident::$assign_method:ident, $inherent:ident) => {
        impl core::ops::$op for $name {
            type Output = Self;

            #[inline]
            fn $method(self, rhs: Self) -> Self {
                self.$inherent(rhs)
            }
        }

        impl core::ops::$assign for $name {
            #[inline]
            fn $assign_method(&mut self, rhs: Self) {
                *self = self.$inherent(rhs);
            }
        }
    };
}

/// Implements `Neg` in terms of an inherent method.
macro_rules! impl_neg {
    ($name:ident, $inherent:ident) => {
        impl core::ops::Neg for $name {
            type Output = Self;

            #[inline]
            fn neg(self) -> Self {
                self.$inherent()
            }
        }
    };
}

/// Declares `ONE` and implements `num_traits::One`.
///
/// Invoked only for types that can represent `1.0`, which excludes
/// [`I0F8`](crate::I0F8) and the [angle family](crate::angle).
macro_rules! impl_one {
    ($name:ident, $bits:expr) => {
        impl $name {
            /// The value `1.0`.
            pub const ONE: Self = Self($bits);
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::One for $name {
            #[inline]
            fn one() -> Self {
                Self::ONE
            }
        }
    };
}
pub(in crate::fixed) use {define_newtype, impl_binop, impl_neg, impl_one, impl_shared};
