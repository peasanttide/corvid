//! How many bits a magnitude occupies.
//!
//! Every caller of these is doing the same thing with the answer: shifting a
//! value into the one binade its fixed-point kernel was fitted to, and shifting
//! the result back afterwards. A reciprocal square root normalizes into
//! `[0.5, 2)`, a quaternion normalization scales the largest component up to
//! just under the top of its word, and a world-scale distance shifts three
//! magnitudes down until their squares still fit an `i128`. The shift is
//! different each time; the question is not.

/// How many bits `value` occupies: zero for zero, otherwise `1 + floor(log2 v)`.
///
/// ```
/// use corvid_bits::bit_length_u32;
///
/// assert_eq!(bit_length_u32(0), 0);
/// assert_eq!(bit_length_u32(0b1011), 4);
/// assert_eq!(bit_length_u32(u32::MAX), 32);
/// ```
#[must_use]
#[inline]
pub const fn bit_length_u32(value: u32) -> u32 {
    u32::BITS - value.leading_zeros()
}

/// How many bits `value` occupies: zero for zero, otherwise `1 + floor(log2 v)`.
///
/// ```
/// use corvid_bits::bit_length_u64;
///
/// assert_eq!(bit_length_u64(0), 0);
/// assert_eq!(bit_length_u64(1 << 40), 41);
/// assert_eq!(bit_length_u64(u64::MAX), 64);
/// ```
#[must_use]
#[inline]
pub const fn bit_length_u64(value: u64) -> u32 {
    u64::BITS - value.leading_zeros()
}

/// How many bits `value` occupies: zero for zero, otherwise `1 + floor(log2 v)`.
///
/// ```
/// use corvid_bits::bit_length_u128;
///
/// assert_eq!(bit_length_u128(0), 0);
/// assert_eq!(bit_length_u128(1 << 100), 101);
/// assert_eq!(bit_length_u128(u128::MAX), 128);
/// ```
#[must_use]
#[inline]
pub const fn bit_length_u128(value: u128) -> u32 {
    u128::BITS - value.leading_zeros()
}

/// How many bits `value`'s magnitude occupies.
///
/// [`i32::MIN`] answers 32, because its magnitude is 2^31 and that needs the
/// thirty-second bit. That is why this takes the magnitude through
/// [`unsigned_abs`](i32::unsigned_abs) rather than through a negation, which
/// would overflow on exactly that value.
///
/// ```
/// use corvid_bits::magnitude_bits_i32;
///
/// assert_eq!(magnitude_bits_i32(0), 0);
/// assert_eq!(magnitude_bits_i32(-8), 4);
/// assert_eq!(magnitude_bits_i32(i32::MIN), 32);
/// ```
#[must_use]
#[inline]
pub const fn magnitude_bits_i32(value: i32) -> u32 {
    bit_length_u32(value.unsigned_abs())
}

/// How many bits `value`'s magnitude occupies.
///
/// [`i64::MIN`] answers 64, for [`magnitude_bits_i32`]'s reason.
///
/// ```
/// use corvid_bits::magnitude_bits_i64;
///
/// assert_eq!(magnitude_bits_i64(-1), 1);
/// assert_eq!(magnitude_bits_i64(i64::MIN), 64);
/// ```
#[must_use]
#[inline]
pub const fn magnitude_bits_i64(value: i64) -> u32 {
    bit_length_u64(value.unsigned_abs())
}

/// How many bits `value`'s magnitude occupies.
///
/// [`i128::MIN`] answers 128, for [`magnitude_bits_i32`]'s reason.
///
/// ```
/// use corvid_bits::magnitude_bits_i128;
///
/// assert_eq!(magnitude_bits_i128(-1), 1);
/// assert_eq!(magnitude_bits_i128(i128::MIN), 128);
/// ```
#[must_use]
#[inline]
pub const fn magnitude_bits_i128(value: i128) -> u32 {
    bit_length_u128(value.unsigned_abs())
}
