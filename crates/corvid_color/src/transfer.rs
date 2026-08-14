//! The sRGB transfer function, as a table.
//!
//! Every colour a game authors is written in sRGB -- a hex code in a palette, a
//! byte in a texture -- and every colour a shader multiplies has to be linear,
//! because light adds and sRGB codes do not. This module is that crossing, and
//! it is the one place in the crate where the two are different things.
//!
//! # Why a table rather than the formula
//!
//! The formula is a power of 2.4, and there is no fixed-point one -- nor a
//! `powf` in `core` to fall back to, so the alternative was a `libm`
//! dependency and a float for two hundred and fifty-six values that are known
//! at compile time. Being a table is also what makes [`decode`] a `const fn`,
//! which is what lets a whole palette be a `const`.
//!
//! [`encode`] is a binary search over the same table followed by one comparison
//! against the midpoint of the interval it lands in. That makes it the exact
//! inverse of [`decode`] for every one of the 256 codes rather than merely
//! close, which `tests/round_trip.rs` asserts for all of them. Merely close is
//! not good enough: a transfer function that misses one code in a hundred moves
//! a golden by one least-significant bit, which is a diff nobody can read.

use corvid_fixed::I16F16;

/// The linear value each of the 256 sRGB codes denotes, as [`I16F16`] bit
/// patterns.
///
/// Generated from the sRGB specification's own piecewise definition -- the
/// linear segment below 0.04045 and `((c + 0.055) / 1.055)^2.4` above it --
/// rounded to Q16 at generation time, so nothing here depends on how a target
/// rounds. The values are strictly increasing, which `tests/round_trip.rs`
/// asserts rather than assumes and which is what makes the search in [`encode`]
/// correct rather than usually right.
///
/// Sixteen fractional bits is the resolution question, and it is settled by the
/// low end: code 1 is 20 steps and code 2 is 40, so the darkest codes -- where
/// the transfer function is steepest and a colour space's precision is usually
/// spent -- are still twenty apart. Nothing here collides.
const DECODED: [i32; 256] = [
    0, 20, 40, 60, 80, 99, 119, 139, 159, 179, 199, 219, 241, 264, 288, 313, 340, 367, 396, 427,
    458, 491, 526, 562, 599, 637, 677, 718, 761, 805, 851, 898, 947, 997, 1_048, 1_101, 1_156,
    1_212, 1_270, 1_330, 1_391, 1_453, 1_517, 1_583, 1_651, 1_720, 1_791, 1_863, 1_937, 2_013,
    2_090, 2_170, 2_250, 2_333, 2_418, 2_504, 2_592, 2_681, 2_773, 2_866, 2_961, 3_058, 3_157,
    3_258, 3_360, 3_464, 3_570, 3_678, 3_788, 3_900, 4_014, 4_129, 4_247, 4_366, 4_488, 4_611,
    4_736, 4_864, 4_993, 5_124, 5_257, 5_392, 5_530, 5_669, 5_810, 5_953, 6_099, 6_246, 6_395,
    6_547, 6_701, 6_856, 7_014, 7_174, 7_336, 7_500, 7_666, 7_834, 8_004, 8_177, 8_352, 8_529,
    8_708, 8_889, 9_072, 9_258, 9_446, 9_636, 9_828, 10_022, 10_219, 10_418, 10_619, 10_822,
    11_028, 11_236, 11_446, 11_658, 11_873, 12_090, 12_309, 12_531, 12_754, 12_981, 13_209, 13_440,
    13_673, 13_909, 14_147, 14_387, 14_629, 14_874, 15_122, 15_372, 15_624, 15_878, 16_135, 16_394,
    16_656, 16_920, 17_187, 17_456, 17_727, 18_001, 18_278, 18_556, 18_838, 19_121, 19_408, 19_696,
    19_988, 20_281, 20_578, 20_876, 21_178, 21_481, 21_788, 22_096, 22_408, 22_722, 23_038, 23_357,
    23_679, 24_003, 24_329, 24_659, 24_991, 25_325, 25_662, 26_002, 26_344, 26_689, 27_036, 27_387,
    27_739, 28_095, 28_453, 28_813, 29_177, 29_543, 29_911, 30_283, 30_657, 31_033, 31_413, 31_795,
    32_180, 32_567, 32_957, 33_350, 33_746, 34_144, 34_545, 34_949, 35_355, 35_765, 36_177, 36_591,
    37_009, 37_429, 37_852, 38_278, 38_707, 39_138, 39_572, 40_009, 40_449, 40_892, 41_337, 41_786,
    42_237, 42_691, 43_147, 43_607, 44_069, 44_534, 45_003, 45_474, 45_947, 46_424, 46_904, 47_386,
    47_871, 48_360, 48_851, 49_345, 49_842, 50_342, 50_844, 51_350, 51_859, 52_370, 52_884, 53_402,
    53_922, 54_445, 54_972, 55_501, 56_033, 56_568, 57_106, 57_647, 58_191, 58_738, 59_288, 59_841,
    60_397, 60_956, 61_518, 62_083, 62_651, 63_222, 63_796, 64_373, 64_953, 65_536,
];

/// The linear value an 8-bit sRGB code denotes.
///
/// Exact, in the sense that matters: [`encode`] takes every one of these values
/// back to the code it came from.
///
/// ```
/// use corvid_color::decode;
/// use corvid_fixed::I16F16;
///
/// // Mid grey in sRGB is not half the light. Getting that backwards is the
/// // single most common colour bug there is.
/// let mid = decode(128);
/// assert!((0.215..0.217).contains(&mid.to_f64()), "{mid:?}");
/// ```
#[must_use]
#[inline]
pub const fn decode(code: u8) -> I16F16 {
    I16F16::from_bits(DECODED[code as usize])
}

/// The 8-bit sRGB code a linear value denotes: the exact inverse of [`decode`]
/// for every code, and the nearest code otherwise.
///
/// Values below zero answer zero and values above one answer 255, because a
/// colour arriving from a shader readback, a file or an out-of-gamut Oklab
/// conversion is not something this crate gets to assume about -- and a
/// fixed-point type has no `NaN` to be a third case, which is one of the things
/// being fixed point buys.
///
/// ```
/// use corvid_color::encode;
/// use corvid_fixed::I16F16;
///
/// assert_eq!(encode(I16F16::from_f64(-1.0)), 0);
/// assert_eq!(encode(I16F16::from_f64(2.0)), 255);
/// ```
#[must_use]
pub const fn encode(linear: I16F16) -> u8 {
    let value = linear.to_bits();
    if value <= 0 {
        return 0;
    }
    if value >= DECODED[255] {
        return 255;
    }
    // `DECODED` is strictly increasing, so this converges on the interval that
    // contains `value`, with `low` below it and `high` above.
    //
    // The bounds are `u8` rather than `usize` because the answer is a `u8`: a
    // narrowing cast at the end would be a truncation the compiler cannot rule
    // out, where a `u8` that never leaves 0..=255 needs no cast at all.
    let mut low = 0u8;
    let mut high = 255u8;
    while low + 1 < high {
        let middle = low.midpoint(high);
        if DECODED[middle as usize] <= value {
            low = middle;
        } else {
            high = middle;
        }
    }
    // Nearest rather than lower. Truncating loses the top half of every
    // interval, which is what makes a round trip miss by one.
    let midpoint = i32::midpoint(DECODED[low as usize], DECODED[high as usize]);
    if value < midpoint { low } else { high }
}
