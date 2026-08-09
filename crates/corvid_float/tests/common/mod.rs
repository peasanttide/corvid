//! Helpers shared by the test binaries.

#![allow(
    unreachable_pub,
    dead_code,
    reason = "each test binary includes this module and uses a different subset of it"
)]
#![allow(
    clippy::cast_precision_loss,
    reason = "the loop counters are small integers being turned into sample points, which is exact for every value they take"
)]

/// How far apart the sampled bit patterns are in the sweeps that walk the whole
/// of `f32`.
///
/// A full sweep is 2^32 software square roots and would turn a test run into a
/// coffee break; a stride of 2^12 leaves a million samples, which is enough to
/// visit every one of the 254 normal exponents, both zeros, both infinities and
/// a wide spread of subnormals. The values a stride skips are the ones between
/// two that were checked, and none of these functions has anywhere to hide
/// between adjacent mantissas.
pub const STRIDE: usize = 1 << 12;

/// Whether two `f32`s are the same answer: the same bits, or both `NaN`.
///
/// A `NaN` payload is not part of what either implementation promises -- the
/// software square root of a negative builds its `NaN` out of an arithmetic
/// operation and the hardware one hands back the platform's -- so holding one to
/// the other's spare mantissa bits would be testing something neither claims.
/// Everything else, including which zero and which infinity, is held exactly.
#[track_caller]
pub fn same(ours: f32, theirs: f32, what: &str) {
    assert!(
        ours.to_bits() == theirs.to_bits() || (ours.is_nan() && theirs.is_nan()),
        "{what}: ours {ours:e} ({:08x}) vs theirs {theirs:e} ({:08x})",
        ours.to_bits(),
        theirs.to_bits()
    );
}

/// [`same`] a word wider.
#[track_caller]
pub fn same_wide(ours: f64, theirs: f64, what: &str) {
    assert!(
        ours.to_bits() == theirs.to_bits() || (ours.is_nan() && theirs.is_nan()),
        "{what}: ours {ours:e} ({:016x}) vs theirs {theirs:e} ({:016x})",
        ours.to_bits(),
        theirs.to_bits()
    );
}

/// The count of representable values between two `f32`s.
///
/// The raw bits are not monotonic across zero -- the sign bit makes `-0.0` the
/// largest pattern rather than the one below `0.0` -- so they are folded onto a
/// signed ordering first. Without that fold a pair straddling zero reads as two
/// billion last bits apart and every tolerance below would be meaningless.
pub fn ulps(ours: f32, theirs: f32) -> i64 {
    fn key(x: f32) -> i64 {
        let bits = x.to_bits();
        if bits >> 31 == 1 {
            -i64::from(bits & 0x7fff_ffff)
        } else {
            i64::from(bits)
        }
    }
    (key(ours) - key(theirs)).abs()
}
