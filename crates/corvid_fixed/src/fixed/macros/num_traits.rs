//! The generators for the `num-traits` integration, which is optional and
//! therefore separate: nothing here is reachable without the feature.

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
            /// The integer part, truncated toward zero.
            ///
            /// Routed through `f64`, so for [`I48F16`](crate::I48F16) -- whose
            /// [`to_f64`](Self::to_f64) is lossy -- the answer can be one
            /// greater than the true integer part, and can exceed the type's
            /// own range. Reach for `to_bits() >> FRAC_BITS` when that
            /// matters.
            #[inline]
            fn to_i64(&self) -> Option<i64> {
                Some(Self::to_f64(*self) as i64)
            }

            /// The integer part, truncated toward zero, or `None` if negative.
            ///
            /// Carries `to_i64`'s `I48F16` caveat.
            #[inline]
            fn to_u64(&self) -> Option<u64> {
                let value = Self::to_f64(*self);
                if value < 0.0 {
                    None
                } else {
                    Some(value as u64)
                }
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
pub(in crate::fixed) use {
    impl_num_traits_arith, impl_num_traits_shared, impl_num_traits_wrapping,
};
