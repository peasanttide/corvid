//! The generator for a point's trait impls: equality, hashing, formatting, and
//! the operators that stand for the inherent methods
//! [`define_point`](super::base::define_point) declares.

macro_rules! define_point_traits {
    ($name:ident, $scalar:ident) => {
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

pub(super) use define_point_traits;
