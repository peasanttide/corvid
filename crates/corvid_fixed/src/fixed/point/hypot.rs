//! The generator for the two hypotenuses.
//!
//! Separate from [`define_fixed_point_math`](super::math::define_fixed_point_math)
//! because the kernel a type reaches for depends on how wide its sum of squares
//! is, and that is the one thing the other generators never have to know: a
//! multiply and a divide are the same code at every width, and a square root is
//! not.

/// Generates [`hypot`](crate::I16F16::hypot) and
/// [`hypot1`](crate::I16F16::hypot1) for one type.
///
/// `sum` is the width two squared bit patterns need, and `root` names the
/// kernel in [`hypot`](super::super::hypot) that takes it. That is a width of
/// its own rather than the `uwide` the multiply uses: an `I8F8` product wants
/// `u32` and a sum of `I8F8` squares fits it too, but sharing the narrow types'
/// kernel with the 32-bit ones is worth more than the word.
macro_rules! define_fixed_point_hypot {
    ($name:ident, $wide:ty, $sum:ty, $frac:expr, $root:ident) => {
        impl $name {
            /// The distance from the origin to the point (`self`, `other`) on
            /// the Euclidean plane, which is the hypotenuse of a right-angle
            /// triangle whose other two sides are `self.abs()` and
            /// `other.abs()`.
            ///
            /// Exact, and correctly rounded. The sum of squares is formed at
            /// full width, so a result that is in range is reached without an
            /// intermediate that is not -- `I24F8::MAX.hypot(I24F8::MAX)` is a
            /// saturation rather than a wrap -- and the square root of that sum
            /// lands on the nearest representable value rather than near it.
            /// Two machines that agree on the arguments agree on the answer.
            ///
            /// Saturating at [`MAX`](Self::MAX) is the only way out of range a
            /// result that cannot be negative has.
            #[must_use]
            #[inline]
            pub const fn hypot(self, other: Self) -> Self {
                let a = self.0.unsigned_abs() as $sum;
                let b = other.0.unsigned_abs() as $sum;
                Self::saturate(super::hypot::$root(a * a + b * b) as $wide)
            }

            /// The same with the other side fixed at one: `sqrt(self^2 + 1)`,
            /// the length of the vector (`self`, `1`).
            ///
            /// This is the shape a slope takes when it becomes a direction and
            /// a tangent takes when it becomes a secant, and it is worth its own
            /// name because one is not always representable -- writing it as
            /// `self.hypot(Self::ONE)` costs [`I0F8`] a constant it does not
            /// have. Exact and correctly rounded for the same reason
            /// [`hypot`](Self::hypot) is.
            ///
            /// The result is never below one, so for [`I0F8`], whose values are
            /// all under `0.5` in magnitude, it always saturates.
            #[must_use]
            #[inline]
            pub const fn hypot1(self) -> Self {
                let a = self.0.unsigned_abs() as $sum;
                let one = (1 as $sum) << $frac;
                Self::saturate(super::hypot::$root(a * a + one * one) as $wide)
            }
        }
    };
}

pub(super) use define_fixed_point_hypot;
