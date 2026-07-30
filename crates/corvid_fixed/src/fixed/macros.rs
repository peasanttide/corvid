//! Code generators shared by the four families.
//!
//! Each family module ([`point`](super::point), [`factor`](super::factor),
//! [`signed`](super::signed), [`angle`](super::angle)) owns the macro that
//! knows its arithmetic, and calls into this module for everything the families
//! have in common: the newtype declaration with its optional derives, bit
//! access, comparison, formatting, and the `num-traits` glue.
//!
//! The generated types are separate structs rather than aliases of one generic
//! type. That keeps the families from mixing — a `Factor16` cannot be added to
//! an `Angle16` — and keeps rustdoc showing concrete signatures.
//!
//! # Contract
//!
//! A family macro must define these before invoking [`impl_shared`]:
//!
//! - `MIN` and `MAX` associated constants.
//! - `const fn cmp_key(self) -> $repr`, the bit pattern used for equality,
//!   ordering, and hashing. It is the identity for every family except the
//!   signed-normalized one, which folds its denormal encoding of `-1.0`.
//! - `const fn to_f64(self) -> f64`.

/// Declares a family newtype, applying the optional interop derives.
///
/// Comparison, hashing, and formatting are deliberately not derived:
/// [`impl_shared`] routes them through `cmp_key` so that the signed-normalized
/// family's two encodings of `-1.0` behave as one value.
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
            #[must_use]
            #[inline]
            pub const fn min(self, other: Self) -> Self {
                if self.cmp_key() <= other.cmp_key() { self } else { other }
            }

            /// Returns the greater of two values.
            #[must_use]
            #[inline]
            pub const fn max(self, other: Self) -> Self {
                if self.cmp_key() >= other.cmp_key() { self } else { other }
            }

            /// Clamps to the inclusive range `min ..= max`.
            ///
            /// Unlike [`Ord::clamp`] this cannot panic: when `min > max` the
            /// bound applied last wins and `max` is returned.
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

            /// The value as an `f32`, correctly rounded.
            ///
            /// Widening to `f64` first means the result is rounded once rather
            /// than twice.
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

/// Declares `ONE` and implements [`num_traits::One`].
///
/// Invoked only for types that can represent `1.0`, which excludes
/// [`I0F8`](crate::I0F8) and the angle family.
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

/// Implements the `num-traits` conversions and bounds every family supports.
macro_rules! impl_num_traits_shared {
    ($name:ident) => {
        #[cfg(feature = "num-traits")]
        impl ::num_traits::Zero for $name {
            #[inline]
            fn zero() -> Self {
                Self::ZERO
            }

            #[inline]
            fn is_zero(&self) -> bool {
                (*self).is_zero()
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::Bounded for $name {
            #[inline]
            fn min_value() -> Self {
                Self::MIN
            }

            #[inline]
            fn max_value() -> Self {
                Self::MAX
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::ToPrimitive for $name {
            #[inline]
            fn to_i64(&self) -> Option<i64> {
                Some(Self::to_f64(*self) as i64)
            }

            #[inline]
            fn to_u64(&self) -> Option<u64> {
                let value = Self::to_f64(*self);
                if value < 0.0 { None } else { Some(value as u64) }
            }

            #[inline]
            fn to_f32(&self) -> Option<f32> {
                Some(Self::to_f32(*self))
            }

            #[inline]
            fn to_f64(&self) -> Option<f64> {
                Some(Self::to_f64(*self))
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::FromPrimitive for $name {
            #[inline]
            fn from_i64(n: i64) -> Option<Self> {
                Self::checked_from_f64(n as f64)
            }

            #[inline]
            fn from_u64(n: u64) -> Option<Self> {
                Self::checked_from_f64(n as f64)
            }

            #[inline]
            fn from_f64(n: f64) -> Option<Self> {
                Self::checked_from_f64(n)
            }
        }
    };
}

/// Implements the `num-traits` checked and saturating operator traits.
macro_rules! impl_num_traits_arith {
    ($name:ident) => {
        #[cfg(feature = "num-traits")]
        impl ::num_traits::CheckedAdd for $name {
            #[inline]
            fn checked_add(&self, rhs: &Self) -> Option<Self> {
                (*self).checked_add(*rhs)
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::CheckedSub for $name {
            #[inline]
            fn checked_sub(&self, rhs: &Self) -> Option<Self> {
                (*self).checked_sub(*rhs)
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::CheckedMul for $name {
            #[inline]
            fn checked_mul(&self, rhs: &Self) -> Option<Self> {
                (*self).checked_mul(*rhs)
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::CheckedDiv for $name {
            #[inline]
            fn checked_div(&self, rhs: &Self) -> Option<Self> {
                (*self).checked_div(*rhs)
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::Saturating for $name {
            #[inline]
            fn saturating_add(self, rhs: Self) -> Self {
                Self::saturating_add(self, rhs)
            }

            #[inline]
            fn saturating_sub(self, rhs: Self) -> Self {
                Self::saturating_sub(self, rhs)
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::SaturatingAdd for $name {
            #[inline]
            fn saturating_add(&self, rhs: &Self) -> Self {
                (*self).saturating_add(*rhs)
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::SaturatingSub for $name {
            #[inline]
            fn saturating_sub(&self, rhs: &Self) -> Self {
                (*self).saturating_sub(*rhs)
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::SaturatingMul for $name {
            #[inline]
            fn saturating_mul(&self, rhs: &Self) -> Self {
                (*self).saturating_mul(*rhs)
            }
        }
    };
}

/// Implements the `num-traits` wrapping operator traits.
///
/// Invoked only for the families whose value space is a modular group: the
/// fixed-point types, where wrapping is two's-complement wraparound, and the
/// angle types, where it is the wraparound of the circle itself.
macro_rules! impl_num_traits_wrapping {
    ($name:ident) => {
        #[cfg(feature = "num-traits")]
        impl ::num_traits::WrappingAdd for $name {
            #[inline]
            fn wrapping_add(&self, rhs: &Self) -> Self {
                (*self).wrapping_add(*rhs)
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::WrappingSub for $name {
            #[inline]
            fn wrapping_sub(&self, rhs: &Self) -> Self {
                (*self).wrapping_sub(*rhs)
            }
        }

        #[cfg(feature = "num-traits")]
        impl ::num_traits::WrappingNeg for $name {
            #[inline]
            fn wrapping_neg(&self) -> Self {
                (*self).wrapping_neg()
            }
        }
    };
}

pub(super) use {
    define_newtype, impl_binop, impl_neg, impl_num_traits_arith, impl_num_traits_shared,
    impl_num_traits_wrapping, impl_one, impl_shared,
};
