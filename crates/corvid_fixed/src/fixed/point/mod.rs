//! General-purpose signed fixed-point numbers.
//!
//! These follow the naming convention of the `fixed` crate: [`I8F8`] is signed,
//! with 8 integer bits and 8 fractional bits. A value `v` denotes
//! `v / 2^FRAC_BITS`, so conversion to and from the raw bits is a shift and
//! every value is an exact multiple of [`DELTA`](I8F8::DELTA).

use super::factor::{Factor8, Factor16, Factor32};
use super::macros::{
    define_newtype, impl_binop, impl_neg, impl_num_traits_arith, impl_num_traits_shared,
    impl_num_traits_wrapping, impl_one, impl_shared,
};
mod hypot;
mod macros;
mod math;
mod round;

use hypot::define_fixed_point_hypot;
use macros::define_fixed_point;
use math::define_fixed_point_math;
use round::define_fixed_point_round;

/// Rounds an `f64` half away from zero, keeping `NaN` at zero.
///
/// The caller casts the result to an integer, where Rust's saturating
/// float-to-int conversion supplies the clamping and the `NaN` behavior.
const fn round_f64(scaled: f64) -> f64 {
    if scaled >= 0.0 {
        scaled + 0.5
    } else {
        scaled - 0.5
    }
}

/// Adds the approximate reciprocal square root to one type.
///
/// Separate from [`define_fixed_point`] because it does not apply to the whole
/// family: the kernel behind it is 32-bit clean, so it can only serve types
/// whose bit pattern fits a `u32`. [`I48F16`] stores an `i64` and gets no
/// `rsqrt_fast` -- narrowing its input first would be a 64-bit operation, which
/// is the one thing this tier promises not to do.
macro_rules! define_rsqrt_fast {
    ($name:ident, $wide:ty, $frac:expr) => {
        impl $name {
            /// The reciprocal square root, approximately.
            ///
            /// The result is within `3.2e-5` relative of the true reciprocal
            /// square root, plus the half step that landing on this type's
            /// resolution costs -- about 15 significant bits, measured
            /// exhaustively. What that comes to in last bits depends on how
            /// fine the type is: [`I0F8`] agrees with [`rsqrt`](Self::rsqrt) on
            /// every input, [`I8F8`] and [`I24F8`] are never more than one step
            /// away, [`I16F16`] reaches 171 steps and [`I2F30`] 56,351.
            ///
            /// # When to reach for it
            ///
            /// Every intermediate fits 32 bits and no product needs a widening
            /// multiply, so this is the version that transcribes into a shader
            /// -- and, on a CPU, the version that skips the 128-bit multiplies
            /// [`rsqrt`](Self::rsqrt) spends on its final step and its exact
            /// rounding correction. That is worth about 3.7x on a 64-bit host;
            /// `cargo bench -p corvid_fixed --bench scalar` measures it.
            ///
            /// Fifteen bits is not a step count that could be raised; it is
            /// where 32-bit arithmetic stops. Newton's residual `1 - n q^2` is a
            /// product of two values that must each fit an operand, so it
            /// carries about 15 bits however many times it is iterated. Reach
            /// for [`rsqrt`](Self::rsqrt) when the last bits matter.
            ///
            /// Zero and negatives saturate to [`MAX`](Self::MAX), and results
            /// past [`MAX`](Self::MAX) saturate too -- matching
            /// [`rsqrt`](Self::rsqrt) in both cases.
            #[must_use]
            #[inline]
            pub const fn rsqrt_fast(self) -> Self {
                if self.0 <= 0 {
                    return Self::MAX;
                }
                Self::saturate(super::rsqrt::rsqrt_fast_bits(self.0 as u32, $frac) as $wide)
            }
        }
    };
}

define_rsqrt_fast!(I0F8, i32, 8);
define_rsqrt_fast!(I8F8, i32, 8);
define_rsqrt_fast!(I24F8, i64, 8);
define_rsqrt_fast!(I16F16, i64, 16);
define_rsqrt_fast!(I2F30, i64, 30);
define_fixed_point! {
    /// A signed fixed-point number with no integer bits and 8 fractional bits.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i8` |
    /// | Range | `-0.5 ..= 0.49609375` |
    /// | Resolution | `1/256`, or `0.00390625` |
    ///
    /// `1.0` falls outside the range, so this is the one type in the crate with
    /// no `ONE` and no `num_traits::One` implementation. Products of two
    /// `I0F8` values are always in range; sums, quotients, and square roots
    /// saturate.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::I0F8;
    ///
    /// let quarter = I0F8::from_f64(0.25);
    /// assert_eq!(quarter.to_bits(), 64);
    /// assert_eq!((quarter * quarter).to_f64(), 0.0625);
    ///
    /// // 0.5 is one step past the top of the range, so sums saturate early.
    /// assert_eq!(quarter + quarter, I0F8::MAX);
    /// assert_eq!(quarter.checked_add(quarter), None);
    /// ```
    I0F8(i8) {
        wide: i32,
        uwide: u32,
        frac: 8,
        factor: Factor8,
    }
}

define_fixed_point_math!(I0F8, i8, i32, u32, 8, Factor8);
define_fixed_point_round!(I0F8, i8, i32, u32, 8, Factor8);
define_fixed_point_hypot!(I0F8, i32, u64, 8, root_u64);

define_fixed_point! {
    /// A signed fixed-point number with 8 integer bits and 8 fractional bits.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i16` |
    /// | Range | `-128.0 ..= 127.99609375` |
    /// | Resolution | `1/256`, or `0.00390625` |
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::I8F8;
    ///
    /// let a = I8F8::from_f64(1.5);
    /// let b = I8F8::from_f64(-0.25);
    /// assert_eq!((a * b).to_f64(), -0.375);
    /// assert_eq!((a / b).to_f64(), -6.0);
    /// assert_eq!(I8F8::from_f64(2.25).sqrt().to_f64(), 1.5);
    ///
    /// // Every value round-trips through f32 and f64 unchanged.
    /// assert_eq!(I8F8::from_f32(a.to_f32()), a);
    ///
    /// // Overflow saturates by default and is detectable on demand.
    /// assert_eq!(I8F8::MAX + I8F8::ONE, I8F8::MAX);
    /// assert_eq!(I8F8::MAX.checked_add(I8F8::ONE), None);
    /// assert_eq!(I8F8::MAX.overflowing_add(I8F8::ONE).1, true);
    /// ```
    I8F8(i16) {
        wide: i32,
        uwide: u32,
        frac: 8,
        factor: Factor16,
    }
}

define_fixed_point_math!(I8F8, i16, i32, u32, 8, Factor16);
define_fixed_point_round!(I8F8, i16, i32, u32, 8, Factor16);
define_fixed_point_hypot!(I8F8, i32, u64, 8, root_u64);

define_fixed_point! {
    /// A signed fixed-point number with 24 integer bits and 8 fractional bits.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i32` |
    /// | Range | `-8388608.0 ..= 8388607.99609375` |
    /// | Resolution | `1/256`, or `0.00390625` |
    ///
    /// The workhorse of the family: wide enough for world-space positions at
    /// millimetre-ish resolution while still hashing as a single `i32`.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::{Factor32, I24F8};
    ///
    /// let start = I24F8::from_f64(-1000.5);
    /// let end = I24F8::from_f64(1000.5);
    /// assert_eq!(start.lerp(end, Factor32::ZERO), start);
    /// assert_eq!(start.lerp(end, Factor32::ONE), end);
    /// assert_eq!(start.lerp(end, Factor32::from_f64(0.5)), I24F8::ZERO);
    ///
    /// assert_eq!(I24F8::from_f64(2.0).sqrt().to_f64(), 1.4140625);
    ///
    /// // f64 is lossless; f32 has too few mantissa bits for the full range.
    /// let wide = I24F8::from_f64(8_000_000.5);
    /// assert_eq!(I24F8::from_f64(wide.to_f64()), wide);
    /// ```
    I24F8(i32) {
        wide: i64,
        uwide: u64,
        frac: 8,
        factor: Factor32,
    }
}

define_fixed_point_math!(I24F8, i32, i64, u64, 8, Factor32);
define_fixed_point_round!(I24F8, i32, i64, u64, 8, Factor32);
define_fixed_point_hypot!(I24F8, i64, u64, 8, root_u64);

define_fixed_point! {
    /// A signed fixed-point number with 16 integer bits and 16 fractional bits.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i32` |
    /// | Range | `-32768.0 ..= 32767.9999847412109375` |
    /// | Resolution | `1/65536`, or about `15.26e-6` |
    ///
    /// The near-field type: 15.26 um is about 30x finer than the ~0.5 mm
    /// threshold at which a headset's wearer perceives a position error, and
    /// +/-32.7 km covers anything a renderer draws at once.
    ///
    /// Shares its 16 fractional bits with [`I48F16`], so converting between the
    /// two is a range check on the integer part with no rounding whatsoever.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::{I16F16, I48F16};
    ///
    /// let near = I16F16::from_f64(1.5);
    /// assert_eq!(near.to_bits(), 98_304);
    ///
    /// // The shared binary scale: widening is the bit pattern itself.
    /// let wide = I48F16::from_bits(i64::from(near.to_bits()));
    /// assert_eq!(wide.to_f64(), near.to_f64());
    /// ```
    I16F16(i32) {
        wide: i64,
        uwide: u64,
        frac: 16,
        factor: Factor32,
    }
}

define_fixed_point_math!(I16F16, i32, i64, u64, 16, Factor32);
define_fixed_point_round!(I16F16, i32, i64, u64, 16, Factor32);
define_fixed_point_hypot!(I16F16, i64, u64, 16, root_u64);

define_fixed_point! {
    /// A signed fixed-point number with 48 integer bits and 16 fractional bits.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i64` |
    /// | Range | `-1.407e14 ..= 1.407e14` |
    /// | Resolution | `1/65536`, or about `15.26e-6` |
    ///
    /// Both range and resolution, and it pays for both in width. +/-1.407e14 m is
    /// roughly 940 AU -- past the Kuiper belt -- while the last bit stays at
    /// 15.26 um. This is the type world-space positions widen into before a
    /// subtraction, which is what makes near-field geometry exact at earth
    /// scale.
    ///
    /// This is the one type in the family whose [`to_f64`](Self::to_f64) is
    /// lossy: 63 magnitude bits exceed `f64`'s 53-bit mantissa.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::I48F16;
    ///
    /// // A camera on the earth's surface, and a point about a millimetre away.
    /// let camera = I48F16::from_f64(6_371_000.0);
    /// let target = camera + I48F16::from_f64(0.001);
    ///
    /// // The difference is exact -- the range is spent on the absolute value,
    /// // not on the offset.
    /// assert_eq!((target - camera).to_bits(), I48F16::from_f64(0.001).to_bits());
    /// ```
    I48F16(i64) {
        wide: i128,
        uwide: u128,
        frac: 16,
        factor: Factor32,
    }
}

define_fixed_point_math!(I48F16, i64, i128, u128, 16, Factor32);
define_fixed_point_round!(I48F16, i64, i128, u128, 16, Factor32);
define_fixed_point_hypot!(I48F16, i128, u128, 16, root_u128);

define_fixed_point! {
    /// A signed fixed-point number with 2 integer bits and 30 fractional bits.
    ///
    /// | | |
    /// |---|---|
    /// | Storage | `i32` |
    /// | Range | `-2.0 ..= 1.999999999068677425384521484375` |
    /// | Resolution | `2^-30`, or about `9.31e-10` |
    ///
    /// The rotation-matrix entry type. [`Signed32`](crate::Signed32) is the
    /// obvious choice for a unit-range value and is not the right one: `SNORM`
    /// divides by `2^31 - 1`, which is not a power of two, so every rotated
    /// vector component would pay a constant division. `I2F30` pays a single
    /// `>> 30` instead, spending one bit of range it does not need -- rotation
    /// entries live in `[-1, 1]` and this type reaches `+/-2` -- to buy it.
    ///
    /// `1.0` is exactly `2^30`, so the identity basis is exact, and the last
    /// bit corresponds to about `5e-8 deg` of angular error.
    ///
    /// # Examples
    ///
    /// ```
    /// use corvid_fixed::I2F30;
    ///
    /// assert_eq!(I2F30::ONE.to_bits(), 1 << 30);
    /// assert_eq!(I2F30::ONE.to_f64(), 1.0);
    ///
    /// // The whole point: scaling by a matrix entry is a shift, not a divide.
    /// let half = I2F30::from_f64(0.5);
    /// assert_eq!((half * half).to_f64(), 0.25);
    /// ```
    I2F30(i32) {
        wide: i64,
        uwide: u64,
        frac: 30,
        factor: Factor32,
    }
}

define_fixed_point_math!(I2F30, i32, i64, u64, 30, Factor32);
define_fixed_point_round!(I2F30, i32, i64, u64, 30, Factor32);
define_fixed_point_hypot!(I2F30, i64, u64, 30, root_u64);

impl_one!(I8F8, 256);
impl_one!(I24F8, 256);
impl_one!(I16F16, 65_536);
impl_one!(I48F16, 65_536);
impl_one!(I2F30, 1 << 30);
