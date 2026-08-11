//! The crossing from a ratio of raw units into a fraction of one.
//!
//! Its own file and its own macro because [`macros`](super::macros) is at the
//! workspace's size limit, and because this is a seam rather than a slice: the
//! arithmetic there operates on values that are already fractions, and the one
//! here is how a reading in somebody else's units -- pixels, detents, encoder
//! counts -- becomes one in the first place.

/// Gives one signed normalized type its ratio constructor.
///
/// Invoked beside [`define_signed`](super::macros::define_signed) rather than
/// from inside it, so that the two files stay independently readable. The
/// generated method reaches for `SCALE`, `round_div` and `saturate`, which are
/// private to this module and so in scope where the macro is invoked.
macro_rules! define_signed_ratio {
    ($name:ident, $wide:ty, $uwide:ty) => {
        impl $name {
            #[doc = concat!("The ratio `numerator / denominator` as a [`", stringify!($name), "`], clamping to `-1.0 ..= 1.0`.")]
            ///
            /// How far along a range a reading is, given the reading and what
            /// a whole range is *in the same units*. A device reports in units
            /// of its own -- pixels of mouse motion, detents of a wheel, counts
            /// off an encoder -- and this is the crossing from those to a
            /// fraction of one, which is what a signed normalized value is.
            ///
            /// Total, and every degenerate case answers rather than panicking:
            /// a whole span or more in either direction is the end of the
            /// range, a denominator of zero is the end of the range too --
            /// every reading is a whole span of nothing -- and `0 / 0` is at
            /// rest. The rounding is the same to-nearest the arithmetic uses,
            /// and it is symmetric, so a reading one way is the same size as
            /// the same reading back.
            ///
            /// ```
            #[doc = concat!("use corvid_fixed::", stringify!($name), ";")]
            ///
            #[doc = concat!("let full = ", stringify!($name), "::saturating_from_ratio(100, 100);")]
            #[doc = concat!("assert_eq!(full, ", stringify!($name), "::MAX);")]
            #[doc = concat!("assert_eq!(", stringify!($name), "::saturating_from_ratio(-100, 100), ", stringify!($name), "::MIN);")]
            #[doc = concat!("assert_eq!(", stringify!($name), "::saturating_from_ratio(0, 100), ", stringify!($name), "::ZERO);")]
            ///
            /// // Past a whole span it stays at the end rather than wrapping.
            #[doc = concat!("assert_eq!(", stringify!($name), "::saturating_from_ratio(400, 100), ", stringify!($name), "::MAX);")]
            ///
            /// // A span of nothing is every span at once, and nothing of it is rest.
            #[doc = concat!("assert_eq!(", stringify!($name), "::saturating_from_ratio(1, 0), ", stringify!($name), "::MAX);")]
            #[doc = concat!("assert_eq!(", stringify!($name), "::saturating_from_ratio(0, 0), ", stringify!($name), "::ZERO);")]
            ///
            /// // Symmetric about zero, at every reading.
            #[doc = concat!("let (up, down) = (", stringify!($name), "::saturating_from_ratio(37, 100), ", stringify!($name), "::saturating_from_ratio(-37, 100));")]
            /// assert_eq!(up, -down);
            /// ```
            #[must_use]
            #[inline]
            pub const fn saturating_from_ratio(numerator: $wide, denominator: $wide) -> Self {
                if numerator == 0 {
                    return Self::ZERO;
                }
                let negative = (numerator < 0) != (denominator < 0);
                let end = if negative { Self::MIN } else { Self::MAX };

                let mut top = numerator.unsigned_abs();
                let mut bottom = denominator.unsigned_abs();
                // A whole span or more -- and a span of zero, which nothing
                // divides into -- is the end of the range. Answering it here is
                // also what bounds the widening below.
                if bottom == 0 || top >= bottom {
                    return end;
                }

                // Now `top < bottom`, so the answer is inside the range and all
                // that is left is the scaling. Both sides come down together
                // until the numerator's widening fits, which costs a step in
                // the last place at magnitudes no device reports and keeps the
                // operation total rather than one bound away from wrapping.
                let widest = <$wide>::MAX as $uwide;
                while top > widest / (Self::SCALE as $uwide) || bottom > widest {
                    top >>= 1;
                    bottom >>= 1;
                }

                let scaled = (top * (Self::SCALE as $uwide)) as $wide;
                let magnitude = Self::round_div(scaled, bottom as $wide);
                Self::saturate(if negative { -magnitude } else { magnitude })
            }
        }
    };
}

pub(super) use define_signed_ratio;
