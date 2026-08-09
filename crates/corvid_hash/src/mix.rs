//! The bijective mixer the digest is built from.
//!
//! Three rounds of xor-shift and multiply by an odd constant. Multiplication by
//! an odd constant is a bijection modulo `2^64`, and xor-shift is a bijection,
//! so the whole function is one -- no input can be lost, and the shift and
//! multiply pair carries entropy from the high bits down and back up.

/// An odd constant with a well-distributed bit pattern. Any odd constant makes
/// the multiply bijective; this one is chosen so no shift-and-multiply round
/// leaves a bit unmoved.
const ODD: u64 = 0xbea2_25f9_eb34_556d;

/// Diffuses every input bit across every output bit, reversibly.
///
/// The shift distances alternate between 32 and 29 so that a bit which has just
/// been folded down by one round is folded across an unrelated boundary by the
/// next, rather than back onto itself. That is the reason for the choice, not a
/// property the tests witness. `tests/avalanche.rs` catches a distance at or
/// below a quarter of a word -- sixteen throughout strays sixteen standard
/// deviations from half at its worst cell, and thirteen and seven alternating
/// pins cells at nothing or at everything -- and it catches one within a hair of
/// the word width, from 57 up. Between those it sees nothing: every uniform
/// distance from 17 to 54 passes, including 48, which is as far above half a
/// word as 16 is below and which the same test rejects.
///
/// What holds these particular distances in place is `tests/golden.rs`. Changing
/// any of the three later ones turns all twenty-seven rows of its tables red;
/// changing this first one turns twenty-six of them red, the exception being the
/// input that equals the seed, which drives the state to `mix(0) = 0` and leaves
/// the digest as `mix(8)` -- and `8 >> s` is zero for every `s` above three.
#[inline]
pub(crate) const fn mix(mut x: u64) -> u64 {
    x ^= x >> 32;
    x = x.wrapping_mul(ODD);
    x ^= x >> 29;
    x = x.wrapping_mul(ODD);
    x ^= x >> 32;
    x = x.wrapping_mul(ODD);
    x ^= x >> 29;
    x
}
