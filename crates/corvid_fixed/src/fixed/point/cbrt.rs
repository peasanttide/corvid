//! The cube and the cube root, for [`I2F30`].
//!
//! Not part of [`define_fixed_point_math`](super::math::define_fixed_point_math)
//! because they do not generalise across the family the way the two square
//! roots do. Solving `g^3 = x` on Q30 bit patterns means squaring a value that
//! has already been shifted up by `2 * FRAC_BITS`, which for thirty fractional
//! bits reaches 2^91 -- past the `i64` that is this type's usual widened
//! intermediate. So the working width here is `i128`, and the pair is written
//! once for the one type that needs it rather than generated for five that
//! do not.
//!
//! It lives in this crate rather than in the caller for the workspace's usual
//! reason: `corvid_color` needs a signed cube root to convert Oklab, and a
//! colour crate reimplementing Newton's method is a second answer to a question
//! this crate already exists to answer.

use super::I2F30;

/// One, as a Q30 bit pattern.
const ONE_Q30: i128 = 1 << 30;

impl I2F30 {
    /// The real cube root.
    ///
    /// Newton's method on `g^3 = x`, worked on the bit patterns so that nothing
    /// rounds through a narrower type on the way. The seed divides the value's
    /// bit length by three, which lands within about 25%; Newton roughly
    /// squares the accuracy each pass, so seven passes reach the last bit with
    /// room to spare.
    ///
    /// **Signed**, unlike [`sqrt`](Self::sqrt), which answers
    /// [`ZERO`](Self::ZERO) for a negative input. A cube root is defined for
    /// negative values and the odd symmetry is the point of using one: Oklab's
    /// definition takes the signed root, and a cone response is negative for a
    /// colour outside the gamut.
    ///
    /// ```
    /// use corvid_fixed::I2F30;
    ///
    /// let eighth = I2F30::from_f64(0.125);
    /// assert_eq!(eighth.cbrt().to_f64(), 0.5);
    ///
    /// // Odd, where the square root is not.
    /// assert_eq!((-eighth).cbrt().to_f64(), -0.5);
    /// assert_eq!((-eighth).sqrt(), I2F30::ZERO);
    /// ```
    #[must_use]
    #[inline]
    pub const fn cbrt(self) -> Self {
        let bits = self.to_bits() as i128;
        if bits == 0 {
            return Self::ZERO;
        }
        let negative = bits < 0;
        let magnitude = bits.unsigned_abs().cast_signed();

        // Solving `g^3 = x` in Q30 means solving `g^3 = bits * 2^60` on the
        // patterns.
        let target = magnitude << 60;

        // A bit length of `n` means the value is about `2^(n-1)`, so its cube
        // root is about `2^((n-1)/3)`.
        let length = 128 - target.leading_zeros();
        let mut guess: i128 = 1 << (length.saturating_sub(1) / 3);

        let mut pass = 0;
        while pass < 7 {
            let square = guess * guess;
            if square == 0 {
                break;
            }
            guess = (2 * guess + target / square) / 3;
            pass += 1;
        }

        let root = narrow(guess);
        Self::from_bits(if negative { -root } else { root })
    }

    /// The cube.
    ///
    /// The inverse of [`cbrt`](Self::cbrt), and exact to the last bit for the
    /// same reason: the two intermediate products are kept at full width and
    /// rounded once at the end, where composing two [`saturating_mul`
    /// ](Self::saturating_mul)s would round twice.
    ///
    /// ```
    /// use corvid_fixed::I2F30;
    ///
    /// let half = I2F30::from_f64(0.5);
    /// assert_eq!(half.cube().to_f64(), 0.125);
    /// assert_eq!(half.cube().cbrt(), half);
    /// ```
    #[must_use]
    #[inline]
    pub const fn cube(self) -> Self {
        let bits = self.to_bits() as i128;
        Self::from_bits(narrow(divide(bits * bits * bits, ONE_Q30 * ONE_Q30)))
    }
}

/// Divides, rounding half away from zero.
#[must_use]
#[inline]
const fn divide(numerator: i128, denominator: i128) -> i128 {
    // `unsigned_abs` rather than `abs`, which overflows on `i128::MIN` -- a
    // value no call site here can reach, and a panic the workspace forbids
    // being one branch away from is not worth the shorter spelling.
    let half = (denominator.unsigned_abs() / 2).cast_signed();
    let bump = if numerator < 0 { -half } else { half };
    (numerator + bump) / denominator
}

/// A wide value brought back to an `i32` pattern, clamping rather than
/// wrapping.
#[must_use]
#[inline]
#[expect(
    clippy::cast_possible_truncation,
    reason = "the value is clamped to i32's range on the two branches above the cast, which is what makes the narrowing exact"
)]
const fn narrow(value: i128) -> i32 {
    if value > i32::MAX as i128 {
        i32::MAX
    } else if value < i32::MIN as i128 {
        i32::MIN
    } else {
        value as i32
    }
}
