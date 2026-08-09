//! What the software implementations cost, measured against the intrinsics
//! they stand in for.
//!
//! This crate's whole trade is that a `const` is worth a slower runtime call,
//! and that trade is only worth making if the answer is the same one. These
//! tests are where "the same one" is given a number.
//!
//! Two numbers, in fact, because the crate makes two different promises. The
//! exact operations — the square root, the four roundings, the two sign
//! operations — are held to the intrinsic's bits, since each of them has a
//! single right answer that IEEE 754 names and both implementations reach it.
//! The transcendentals are held to a count of last bits, since neither the
//! software one nor the platform's libm is correctly rounded and pretending
//! otherwise would produce a test that passes on this machine only.

#![allow(
    clippy::float_cmp,
    reason = "exact equality is the assertion: `floor`, `ceil`, `trunc`, `round` and `abs` are bit-for-bit operations, and a tolerance here would hide the one thing these tests exist to catch"
)]
#![allow(
    clippy::cast_precision_loss,
    reason = "the loop counters are small integers being turned into the sample points, which is the standard way to sweep a range and is exact for every value they take"
)]
#![allow(
    clippy::cast_possible_truncation,
    reason = "one test deliberately spells out the `+0.5`-then-truncate rounding that a const conversion reaches for when it cannot call `wide::round`, and the truncation is the behaviour under test"
)]
#![allow(
    clippy::suboptimal_flops,
    reason = "the tolerance expressions are written as `scale * relative + absolute` because that is how a tolerance reads; folding them into a `mul_add` would be faster and less legible in code whose job is to be read"
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
const STRIDE: usize = 1 << 12;

/// Whether two `f32`s are the same answer: the same bits, or both `NaN`.
///
/// A `NaN` payload is not part of what either implementation promises — the
/// software square root of a negative builds its `NaN` out of an arithmetic
/// operation and the hardware one hands back the platform's — so holding one to
/// the other's spare mantissa bits would be testing something neither claims.
/// Everything else, including which zero and which infinity, is held exactly.
#[track_caller]
fn same(ours: f32, theirs: f32, what: &str) {
    assert!(
        ours.to_bits() == theirs.to_bits() || (ours.is_nan() && theirs.is_nan()),
        "{what}: ours {ours:e} ({:08x}) vs theirs {theirs:e} ({:08x})",
        ours.to_bits(),
        theirs.to_bits()
    );
}

/// [`same`] a word wider.
#[track_caller]
fn same_wide(ours: f64, theirs: f64, what: &str) {
    assert!(
        ours.to_bits() == theirs.to_bits() || (ours.is_nan() && theirs.is_nan()),
        "{what}: ours {ours:e} ({:016x}) vs theirs {theirs:e} ({:016x})",
        ours.to_bits(),
        theirs.to_bits()
    );
}

/// The count of representable values between two `f32`s.
///
/// The raw bits are not monotonic across zero — the sign bit makes `-0.0` the
/// largest pattern rather than the one below `0.0` — so they are folded onto a
/// signed ordering first. Without that fold a pair straddling zero reads as two
/// billion last bits apart and every tolerance below would be meaningless.
fn ulps(ours: f32, theirs: f32) -> i64 {
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

/// Bit for bit, and over bit patterns rather than over a range of values.
///
/// A square root is one of the five operations IEEE 754 requires to be
/// correctly rounded, so there is an exact answer to hold this to rather than a
/// tolerance — and a tolerance would let a last-bit regression through in the
/// function the composed ones are all built out of. Sweeping bit patterns is
/// what reaches the subnormals and `MAX`; stepping by a sixteenth, as an
/// earlier version of this test did, never leaves a handful of exponents.
#[test]
fn square_roots_are_bit_for_bit_the_intrinsic() {
    for bits in (0..=u32::MAX).step_by(STRIDE) {
        let x = f32::from_bits(bits);
        same(corvid_float::sqrt(x), x.sqrt(), "sqrt");
    }
}

/// The documented behaviour at and below zero, which the sweep above reaches
/// only by accident and which the doc comment states outright.
#[test]
fn a_square_root_of_a_negative_is_a_nan_and_of_a_negative_zero_is_a_negative_zero() {
    assert!(corvid_float::sqrt(-1.0).is_nan());
    assert!(corvid_float::sqrt(f32::MIN).is_nan());
    assert!(corvid_float::sqrt(f32::NEG_INFINITY).is_nan());
    assert!(corvid_float::sqrt(f32::NAN).is_nan());
    same(corvid_float::sqrt(-0.0), (-0.0f32).sqrt(), "sqrt(-0.0)");
    assert!(corvid_float::sqrt(-0.0).is_sign_negative());
    same(corvid_float::sqrt(0.0), 0.0, "sqrt(0.0)");
    same(
        corvid_float::sqrt(f32::INFINITY),
        f32::INFINITY,
        "sqrt(inf)",
    );
}

/// The four roundings and the two sign operations, over the same sweep of bit
/// patterns and to the same bit-for-bit standard.
///
/// This is the test that says the composed [`corvid_float::ceil`] — `-floor(-x)`
/// — is the ceiling rather than an approximation of it, and that the roundings
/// have not quietly lost a sign on a zero. It is also where a `-0.0` regression
/// would show, which an `assert_eq!` between two floats cannot see: `-0.0 ==
/// 0.0` is true and their bits are not.
#[test]
fn the_exact_operations_are_bit_for_bit_the_intrinsics() {
    for bits in (0..=u32::MAX).step_by(STRIDE) {
        let x = f32::from_bits(bits);
        same(corvid_float::floor(x), x.floor(), "floor");
        same(corvid_float::ceil(x), x.ceil(), "ceil");
        same(corvid_float::trunc(x), x.trunc(), "trunc");
        same(corvid_float::round(x), x.round(), "round");
        same(corvid_float::abs(x), x.abs(), "abs");
        same(
            corvid_float::copysign(x, -1.0),
            x.copysign(-1.0),
            "copysign-",
        );
        same(corvid_float::copysign(x, 1.0), x.copysign(1.0), "copysign+");
    }
}

/// The values a strided sweep can miss, named rather than sampled.
#[test]
fn the_specials_are_the_intrinsics_specials() {
    let specials = [
        0.0_f32,
        -0.0,
        1.0,
        -1.0,
        0.5,
        -0.5,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        f32::MIN_POSITIVE,
        -f32::MIN_POSITIVE,
        f32::from_bits(1),
        f32::from_bits(0x8000_0001),
        f32::MAX,
        f32::MIN,
        f32::from_bits(0.5_f32.to_bits() - 1),
    ];
    for x in specials {
        same(corvid_float::floor(x), x.floor(), "floor");
        same(corvid_float::ceil(x), x.ceil(), "ceil");
        same(corvid_float::trunc(x), x.trunc(), "trunc");
        same(corvid_float::round(x), x.round(), "round");
        same(corvid_float::abs(x), x.abs(), "abs");
        same(corvid_float::sqrt(x), x.sqrt(), "sqrt");
        same(
            corvid_float::copysign(x, -2.0),
            x.copysign(-2.0),
            "copysign",
        );
    }
}

/// The doc comment for [`corvid_float::abs`] says the sign-bit spelling is what
/// makes it right for `-0.0` and for `NaN`, and both of those are exactly the
/// values a comparison-based `abs` gets wrong. Neither survives an
/// `assert_eq!`, so both are checked on their bits.
#[test]
fn the_magnitude_is_a_sign_bit_and_not_a_comparison() {
    assert_eq!(corvid_float::abs(-0.0).to_bits(), 0.0_f32.to_bits());
    assert_eq!(corvid_float::abs(0.0).to_bits(), 0.0_f32.to_bits());
    let negative_nan = f32::from_bits(0xffc0_0000);
    assert!(corvid_float::abs(negative_nan).is_nan());
    assert!(corvid_float::abs(negative_nan).is_sign_positive());
    assert_eq!(corvid_float::abs(f32::NEG_INFINITY), f32::INFINITY);
}

/// Over a couple of turns either side of zero, which is the range a frustum's
/// half-angle and a gain curve actually live in. `const_soft_float`'s argument
/// reduction is `rem_pio2`, so this is also what checks that the reduction is
/// there at all.
///
/// A last bit, not the `1e-6` an earlier version of this test allowed: the
/// README claims these agree with the intrinsic to within one representable
/// value and this is the assertion that claim rests on. Eight last bits of slack
/// would let a real regression through unnoticed.
#[test]
fn sines_and_cosines_match_the_intrinsic_to_a_last_bit() {
    for step in -800i32..800 {
        let x = step as f32 / 64.0;
        let (ours, theirs) = (corvid_float::sin(x), x.sin());
        assert!(ulps(ours, theirs) <= 1, "sin({x}): {ours} vs {theirs}");
        let (ours, theirs) = (corvid_float::cos(x), x.cos());
        assert!(ulps(ours, theirs) <= 1, "cos({x}): {ours} vs {theirs}");
    }
}

/// Well past where the reduction has any easy job left. A `sin` that dropped
/// `rem_pio2` would still pass the sweep above and would be nonsense here.
#[test]
fn sines_and_cosines_survive_an_argument_no_reduction_wants() {
    for x in [1.0e7_f32, 1.0e12, 1.0e20, f32::MAX] {
        assert!(ulps(corvid_float::sin(x), x.sin()) <= 1, "sin({x:e})");
        assert!(ulps(corvid_float::cos(x), x.cos()) <= 1, "cos({x:e})");
    }
    assert!(corvid_float::sin(f32::INFINITY).is_nan());
    assert!(corvid_float::cos(f32::INFINITY).is_nan());
    assert!(corvid_float::sin(f32::NAN).is_nan());
}

/// The composed one. Away from the poles, where both answers are enormous and
/// neither is useful.
///
/// Two last bits rather than one, because a quotient of two values that are each
/// a last bit out is two last bits out. That is arithmetic rather than slack.
#[test]
fn tangents_match_the_intrinsic_away_from_the_poles() {
    for step in -100i32..100 {
        let x = step as f32 / 128.0;
        let (ours, theirs) = (corvid_float::tan(x), x.tan());
        assert!(ulps(ours, theirs) <= 2, "tan({x}): {ours} vs {theirs}");
    }
}

/// The trap [`corvid_float::tan`]'s doc comment is about.
///
/// No `f32` is π/2, so the cosine never actually reaches zero and the tangent
/// at the pole is not an infinity — it is a large finite number whose sign
/// depends on which side of π/2 the argument rounded to. `FRAC_PI_2` rounds
/// just past, so the answer there is negative, and a caller screening its
/// frustum for a non-finite focal length would pass a mirrored one straight
/// through. This test exists so that if the crate ever does start returning an
/// infinity there, the doc comment gets rewritten with it.
#[test]
fn a_tangent_at_the_pole_is_large_finite_and_carries_the_sign_of_the_rounding() {
    let over = corvid_float::consts::FRAC_PI_2;
    let under = f32::from_bits(over.to_bits() - 1);

    assert!(over > core::f32::consts::FRAC_PI_2 || under < over);
    for x in [over, under] {
        let ours = corvid_float::tan(x);
        assert!(ours.is_finite(), "tan({x}) = {ours} should be finite");
        assert!(ours.abs() > 1.0e6, "tan({x}) = {ours} should be enormous");
        assert!(ulps(ours, x.tan()) <= 2, "tan({x}): {ours} vs {}", x.tan());
    }
    assert!(
        corvid_float::tan(over) < 0.0,
        "π/2 rounds up, so tan is below"
    );
    assert!(
        corvid_float::tan(under) > 0.0,
        "and the value below it is above"
    );
}

/// The reciprocal's documented edge, which is a real infinity because it is a
/// real division by a real zero — unlike the tangent's.
#[test]
fn a_reciprocal_of_zero_is_an_infinity_rather_than_a_panic() {
    assert_eq!(corvid_float::recip(0.0), f32::INFINITY);
    assert_eq!(corvid_float::recip(-0.0), f32::NEG_INFINITY);
    assert_eq!(corvid_float::recip(f32::INFINITY), 0.0);
    assert!(corvid_float::recip(f32::NAN).is_nan());
    assert_eq!(corvid_float::recip(4.0), 0.25);
    assert_eq!(corvid_float::wide::recip(0.0), f64::INFINITY);
}

/// An integer power, against the answer worked out exactly.
///
/// # Why not against the intrinsic
///
/// Because `f32::powi` is not an answer. It lowers to `llvm.powi`, which is not
/// correctly rounded and is free to differ between targets — and it does: at
/// `powi(-5.25, -18)` it lands three ulps from the true value on
/// `x86_64-pc-windows-msvc` and on the true value on
/// `x86_64-unknown-linux-gnu`. Holding this crate to it would be holding a
/// deterministic function to a non-deterministic one, so the test would say
/// different things on different machines and none of them about this crate.
///
/// [`exact_powi`] works the answer out instead. `(step / 8)^n` is a rational
/// whose parts are a power of an odd integer and a power of two, so once the
/// common twos are cancelled the correctly rounded `f32` is an integer division
/// and a remainder to settle the tie on. The odd part reaches 113 bits at
/// `49^20` — too wide to shift a numerator above and still fit a `u128` — so
/// the reciprocal is taken by [`reciprocal`]'s long division, which never forms
/// a number wider than one doubling of the divisor. There is no floating point
/// anywhere in either, so nothing in them can vary by target.
///
/// Cross-checked against exact rational arithmetic in Python over all four
/// thousand samples of this sweep: every one agrees bit for bit.
///
/// # What the bound says
///
/// That this crate's `powi` is within `1 + ilog2(|n|)` of the true value, which
/// is the shape of the algorithm: binary exponentiation is about `log2(|n|)`
/// squarings and as many multiplies again, every one of them rounds, so the
/// distance from the truth grows with the length of the chain and with nothing
/// else. `n = 0` and `n = ±1` are held exactly, which is where no chain runs.
///
/// Measured over this sweep, and the same on every target because both sides
/// are: 814 of the 4000 samples are not the correctly rounded value, the worst
/// is three ulps at `powi(-5.25, -18)` against a bound of five there, and
/// nothing below `|n| = 10` is out by more than one.
///
/// **This is a measurement, not a target.** A `powi` that carried a few guard
/// bits through the chain would be correctly rounded everywhere and this bound
/// would become `== 0`; the bound is here because the implementation does not,
/// not because three ulps is a thing worth promising.
#[test]
fn integer_powers_are_within_a_chain_s_worth_of_the_exact_answer() {
    /// How far a chain of `|n|` multiplications may carry this crate's answer
    /// from the true one.
    fn allowed(n: i32) -> i64 {
        1 + i64::from(n.unsigned_abs().max(1).ilog2())
    }

    for step in -50i32..50 {
        let x = step as f32 / 8.0;
        for n in -20i32..20 {
            let (ours, truth) = (corvid_float::powi(x, n), exact_powi(step, n));
            let (apart, bound) = (ulps(ours, truth), allowed(n));
            assert!(
                apart <= bound,
                "powi({x}, {n}): {ours:e} against the exact {truth:e} — {apart} ulps out, \
                 {bound} allowed"
            );
        }
    }

    // The edges, where the answer is exact and there is nothing to be out by.
    // `i32::MIN` is the one with no positive counterpart to take a magnitude of.
    same(corvid_float::powi(2.0, 0), 1.0, "powi(2, 0)");
    same(corvid_float::powi(0.0, 0), 1.0, "powi(0, 0)");
    same(corvid_float::powi(2.0, i32::MIN), 0.0, "powi(2, i32::MIN)");
    same(
        corvid_float::powi(2.0, i32::MAX),
        f32::INFINITY,
        "powi(2, i32::MAX)",
    );
}

/// The first `bits` significant bits of `1 / divisor`, how far below the point
/// they sit, and whether anything was left over.
///
/// Ordinary long division in binary: at each place the remainder doubles and
/// the divisor is taken out of it if it fits. The remainder is always below the
/// divisor, so the widest number this ever holds is one doubling of one — which
/// is what lets it divide by a 113-bit `power` that no shifted numerator could
/// reach.
///
/// The leftover remainder is the whole of what "and a bit more" means: a true
/// value strictly above the quotient cannot be an exact tie, however the
/// dropped bits happen to look.
const fn reciprocal(divisor: u128, bits: u32) -> (u128, i64, bool) {
    let (mut quotient, mut remainder, mut places) = (0_u128, 1_u128, 0_i64);
    while 128 - quotient.leading_zeros() < bits {
        remainder <<= 1;
        quotient <<= 1;
        places += 1;
        if remainder >= divisor {
            remainder -= divisor;
            quotient += 1;
        }
    }
    (quotient, places, remainder > 0)
}

/// The correctly rounded `f32` nearest to `(step / 8)^n`, worked out exactly.
///
/// `step / 8` is `odd * 2^k / 8` for some odd `odd`, so `(step / 8)^n` is
/// `odd^|n|` over a power of two, or its reciprocal. `odd` is at most 25 here
/// and `|n|` at most 20, so `odd^|n|` is under 2^93 and every quantity below
/// fits a `u128` with room to shift in. There is no floating point anywhere in
/// this function, so it answers the same on every target.
///
/// Ties round to even, which is what IEEE 754 asks for and what the `f32` this
/// is compared against was rounded by.
fn exact_powi(step: i32, n: i32) -> f32 {
    if n == 0 {
        return 1.0;
    }
    if step == 0 {
        return if n > 0 { 0.0 } else { f32::INFINITY };
    }

    let negative = step < 0 && n % 2 != 0;
    let magnitude = u128::from(step.unsigned_abs());
    let twos = magnitude.trailing_zeros();
    let odd = magnitude >> twos;
    let times = n.unsigned_abs();

    // `odd^|n|`, and the power of two the eighths and the even part leave over.
    let power = odd.pow(times);
    let scale = if n > 0 {
        (i64::from(twos) - 3) * i64::from(times)
    } else {
        (3 - i64::from(twos)) * i64::from(times)
    };

    // The whole value as `mantissa * 2^exponent`, with the mantissa an exact
    // integer for a power and an integer plus a remainder for a reciprocal.
    // `sticky` is set when the true value lies strictly above that integer,
    // which is what stops a division's leftovers reading as an exact tie.
    let (mantissa, exponent, sticky) = if n > 0 {
        (power, scale, false)
    } else {
        // Long division rather than `(1 << s) / power`: `power` reaches 113
        // bits at `49^20`, and the shift that would put twenty-six quotient
        // bits above it does not fit a `u128`. Dividing a place at a time never
        // forms that number — the remainder stays below the divisor, so the
        // widest value here is one doubling of it.
        let (quotient, places, rest) = reciprocal(power, 26);
        (quotient, scale - places, rest)
    };

    // Round that to exactly 24 bits, nearest, ties to even.
    let bits = i64::from(128 - mantissa.leading_zeros());
    let (mut significand, mut exponent) = if bits > 24 {
        let drop = bits - 24;
        #[expect(
            clippy::cast_sign_loss,
            reason = "`drop` is `bits - 24` inside the branch where `bits > 24`, so it is positive"
        )]
        let dropped = drop as u32;
        let cut = mantissa & ((1 << dropped) - 1);
        let half = 1_u128 << (dropped - 1);
        let mut significand = mantissa >> dropped;
        if cut > half || (cut == half && (sticky || significand & 1 == 1)) {
            significand += 1;
        }
        (significand, exponent + drop)
    } else {
        #[expect(
            clippy::cast_sign_loss,
            reason = "`24 - bits` is positive inside the branch where `bits <= 24`"
        )]
        let up = (24 - bits) as u32;
        (mantissa << up, exponent - (24 - bits))
    };
    // A carry out of the top: 0xffffff + 1 is 0x1000000, one bit wider.
    if significand == 1 << 24 {
        significand >>= 1;
        exponent += 1;
    }

    // Assemble. Everything this sweep reaches is a normal `f32`; a value that
    // is not says so rather than folding silently to zero or an infinity.
    let biased = exponent + 23 + 127;
    assert!(
        (1..=254).contains(&biased),
        "(({step}) / 8)^{n} is outside the normal range this reference covers"
    );
    // Both conversions are inside the ranges the assertion above and the
    // twenty-four-bit significand guarantee, so the fallback is unreachable and
    // is written rather than unwrapped.
    let exponent_field = u32::try_from(biased).unwrap_or(0);
    let mantissa_field = u32::try_from(significand & 0x007f_ffff).unwrap_or(0);
    let value = f32::from_bits((exponent_field << 23) | mantissa_field);
    if negative { -value } else { value }
}

#[test]
fn the_composed_rounding_matches_the_intrinsics() {
    for step in -400i32..400 {
        let x = step as f32 / 8.0;
        same(corvid_float::floor(x), x.floor(), "floor");
        same(corvid_float::ceil(x), x.ceil(), "ceil");
        same(corvid_float::trunc(x), x.trunc(), "trunc");
        same(corvid_float::round(x), x.round(), "round");
        same(corvid_float::abs(x), x.abs(), "abs");
    }
}

/// `ceil` is `-floor(-x)`, and the identity is exact because a negation is a
/// sign bit. The one value where a sloppier implementation would disagree with
/// the intrinsic is the negative zero it produces from an input in `(-1, 0)`.
#[test]
fn the_composed_ceiling_keeps_the_intrinsic_s_negative_zero() {
    let ours = corvid_float::ceil(-0.5);
    assert_eq!(ours, (-0.5f32).ceil());
    assert!(ours.is_sign_negative(), "ceil(-0.5) should be -0.0");
}

/// The other half-step trap: a `round` written as `trunc(x + 0.5)` answers one
/// for the value just below a half, because the addition rounds up before the
/// truncation ever runs. `const_soft_float` subtracts a quarter of an epsilon
/// from the half to avoid it, and this is the value that says so.
#[test]
fn rounding_the_value_just_below_a_half_gives_zero() {
    let just_under = f32::from_bits(0.5_f32.to_bits() - 1);
    assert_eq!(corvid_float::round(just_under), 0.0);
    assert_eq!(corvid_float::round(just_under), just_under.round());

    let wide_just_under = 0.499_999_999_999_999_94_f64;
    assert_eq!(corvid_float::wide::round(wide_just_under), 0.0);
    assert_eq!(
        corvid_float::wide::round(wide_just_under),
        wide_just_under.round()
    );
    // The spelling `corvid_fixed`'s `const` conversions use instead, because
    // they do not depend on this crate: add a half, let the cast truncate. It
    // answers one, which is the difference `wide::round`'s doc comment records.
    assert_eq!((wide_just_under + 0.5) as i64, 1);
}

/// Unlike [`f32::clamp`], which panics when its bounds cross. The workspace
/// forbids a panic in a library, so the upper bound simply wins — and `NaN`
/// falls through to the low bound, which is what turns a gain that has gone
/// wrong into silence rather than into full volume.
#[test]
fn clamping_does_not_panic_on_crossed_bounds_or_nan() {
    // The ordinary case, from both directions.
    assert_eq!(corvid_float::clamp(5.0, 0.0, 1.0), 1.0);
    assert_eq!(corvid_float::clamp(-5.0, 0.0, 1.0), 0.0);
    assert_eq!(corvid_float::clamp(0.5, 0.0, 1.0), 0.5);

    // Crossed: `high` is tested first, so anything above it comes back as it.
    assert_eq!(corvid_float::clamp(0.5, 1.0, 0.0), 0.0);
    assert_eq!(corvid_float::clamp(-1.0, 1.0, 0.0), 1.0);

    assert_eq!(corvid_float::clamp(f32::NAN, 2.0, 3.0), 2.0);
    assert_eq!(corvid_float::wide::clamp(f64::NAN, 2.0, 3.0), 2.0);

    // The infinities are ordinary values to this one: above `high` and below
    // `low` respectively. `clamp_finite` is the one that reads them as faults.
    assert_eq!(corvid_float::clamp(f32::INFINITY, 0.0, 1.0), 1.0);
    assert_eq!(corvid_float::clamp(f32::NEG_INFINITY, 0.0, 1.0), 0.0);
}

/// The mixer's clamp. Everything non-finite comes back as `low`, which is
/// silence, and everything finite is clamped the ordinary way.
#[test]
fn clamping_finitely_sends_every_non_finite_to_the_low_bound() {
    assert_eq!(corvid_float::clamp_finite(f32::INFINITY, 0.0, 1.0), 0.0);
    assert_eq!(corvid_float::clamp_finite(f32::NEG_INFINITY, 0.0, 1.0), 0.0);
    assert_eq!(corvid_float::clamp_finite(f32::NAN, 0.0, 1.0), 0.0);

    assert_eq!(corvid_float::clamp_finite(0.5, 0.0, 1.0), 0.5);
    assert_eq!(corvid_float::clamp_finite(-5.0, 0.0, 1.0), 0.0);
    assert_eq!(corvid_float::clamp_finite(5.0, 0.0, 1.0), 1.0);
    // Finite, however large: `f32::MAX` is a value and not a fault.
    assert_eq!(corvid_float::clamp_finite(f32::MAX, 0.0, 1.0), 1.0);

    // The low bound is returned as written, so a mixer asking for silence in a
    // range that does not contain zero gets the bottom of its range.
    assert_eq!(corvid_float::clamp_finite(f32::NAN, -1.0, 1.0), -1.0);
}

/// The two clamps differ in two places and the doc comments name both: what an
/// infinity does, and — because they test their bounds in opposite orders —
/// what crossed bounds do.
#[test]
fn the_two_clamps_part_company_on_infinities_and_on_crossed_bounds() {
    assert_eq!(corvid_float::clamp(f32::INFINITY, 0.0, 1.0), 1.0);
    assert_eq!(corvid_float::clamp_finite(f32::INFINITY, 0.0, 1.0), 0.0);

    // `clamp` tests `high` first, `clamp_finite` tests `low` first, so a value
    // between crossed bounds falls out of opposite arms.
    assert_eq!(corvid_float::clamp(0.5, 1.0, 0.0), 0.0);
    assert_eq!(corvid_float::clamp_finite(0.5, 1.0, 0.0), 1.0);

    // Where they still agree: below both, and above both.
    assert_eq!(corvid_float::clamp(-1.0, 1.0, 0.0), 1.0);
    assert_eq!(corvid_float::clamp_finite(-1.0, 1.0, 0.0), 1.0);
    assert_eq!(corvid_float::clamp(2.0, 1.0, 0.0), 0.0);
    assert_eq!(corvid_float::clamp_finite(2.0, 1.0, 0.0), 0.0);
}

#[test]
fn hypotenuses_match_the_intrinsic_in_the_range_a_camera_works_in() {
    for x in -20i32..20 {
        for y in -20i32..20 {
            let (x, y) = (x as f32 / 4.0, y as f32 / 4.0);
            let (ours, theirs) = (corvid_float::hypot(x, y), x.hypot(y));
            assert!(
                (ours - theirs).abs() <= theirs * 1e-6 + 1e-7,
                "hypot({x}, {y}): {ours} vs {theirs}"
            );
        }
    }
}

/// And the two places outside that range where it does not, both of which the
/// doc comment names. Forming the squares before the root is the whole reason,
/// and a future implementation that scales its arguments first would fail this
/// test — which is the point: the doc comment would have to change with it.
#[test]
fn hypotenuses_overflow_and_collapse_where_the_documentation_says_they_do() {
    // Above the top: the square overflows, the root of an infinity is one.
    assert!(corvid_float::hypot(1.9e19, 0.0).is_infinite());
    assert!(1.9e19_f32.hypot(0.0).is_finite());
    // And just inside it, where the square still fits.
    assert_eq!(corvid_float::hypot(1.8e19, 0.0), 1.8e19);

    // Below the bottom: the square is zero, and so is its root. This is the
    // quieter failure, which is why it is written down.
    assert_eq!(
        corvid_float::hypot(f32::MIN_POSITIVE, f32::MIN_POSITIVE),
        0.0
    );
    assert!(f32::MIN_POSITIVE.hypot(f32::MIN_POSITIVE) > 0.0);

    // The wide half has the same shape of failure, far enough out that no
    // caller reaches it.
    assert!(corvid_float::wide::hypot(1.4e154, 0.0).is_infinite());
    assert!(corvid_float::wide::hypot(1.3e154, 0.0).is_finite());
}

/// The narrowing, and what it does with a value that will not fit.
#[test]
fn demoting_narrows_and_saturates_to_an_infinity() {
    assert_eq!(corvid_float::demote(1.0 / 3.0), 1.0_f32 / 3.0);
    assert_eq!(corvid_float::demote(f64::from(f32::MAX)), f32::MAX);
    assert_eq!(corvid_float::demote(1e300), f32::INFINITY);
    assert_eq!(corvid_float::demote(-1e300), f32::NEG_INFINITY);
    assert_eq!(corvid_float::demote(1e-300), 0.0);
    assert!(corvid_float::demote(f64::NAN).is_nan());
    same(corvid_float::demote(-0.0), -0.0, "demote(-0.0)");

    // Which is the pairing the doc comment claims: an overflow on the way down
    // arrives as an infinity, and `clamp_finite` is what turns it back into a
    // number a device can take.
    assert_eq!(
        corvid_float::clamp_finite(corvid_float::demote(1e300), 0.0, 1.0),
        0.0
    );
}

/// The wide half, spot-checked the same way. It is the same implementation a
/// word wider, so the point here is that the module exists and is wired up
/// rather than that the algorithm is different.
#[test]
fn the_wide_half_matches_its_intrinsics_too() {
    for step in -200i32..200 {
        let x = f64::from(step) / 16.0;
        assert!((corvid_float::wide::sin(x) - x.sin()).abs() < 1e-12);
        assert!((corvid_float::wide::sqrt(x.abs()) - x.abs().sqrt()).abs() < 1e-12);
        same_wide(corvid_float::wide::round(x), x.round(), "wide round");
        same_wide(corvid_float::wide::floor(x), x.floor(), "wide floor");
        same_wide(corvid_float::wide::ceil(x), x.ceil(), "wide ceil");
        same_wide(corvid_float::wide::trunc(x), x.trunc(), "wide trunc");
        same_wide(corvid_float::wide::abs(x), x.abs(), "wide abs");
    }
    for x in [
        0.0_f64,
        -0.0,
        -0.5,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        f64::MIN_POSITIVE,
        f64::MAX,
        f64::MIN,
    ] {
        same_wide(corvid_float::wide::floor(x), x.floor(), "wide floor");
        same_wide(corvid_float::wide::ceil(x), x.ceil(), "wide ceil");
        same_wide(corvid_float::wide::round(x), x.round(), "wide round");
        same_wide(corvid_float::wide::trunc(x), x.trunc(), "wide trunc");
        same_wide(corvid_float::wide::abs(x), x.abs(), "wide abs");
        same_wide(corvid_float::wide::sqrt(x), x.sqrt(), "wide sqrt");
    }
    same_wide(
        corvid_float::wide::sqrt(2.0),
        2.0_f64.sqrt(),
        "wide sqrt(2)",
    );
    same_wide(
        corvid_float::wide::copysign(3.0, -1.0),
        3.0_f64.copysign(-1.0),
        "wide copysign",
    );
}

/// The claim the crate is named for. If any of these stops being `const` the
/// build fails here rather than silently at the call site that wanted it.
///
/// Exhaustive on purpose, and it has to be kept that way: a `const` binding is
/// the only thing that can witness constness, so a function this list forgets
/// is a function whose constness nothing checks. This covers the sixteen names
/// at the root and its sibling below covers the fourteen in `wide`; a new
/// export is not finished until it appears in one of them.
#[test]
fn every_function_is_const() {
    const SQRT: f32 = corvid_float::sqrt(2.0);
    const SIN: f32 = corvid_float::sin(1.0);
    const COS: f32 = corvid_float::cos(1.0);
    const TAN: f32 = corvid_float::tan(0.5);
    const RECIP: f32 = corvid_float::recip(4.0);
    const HYPOT: f32 = corvid_float::hypot(3.0, 4.0);
    const POWI: f32 = corvid_float::powi(2.0, 10);
    const CEIL: f32 = corvid_float::ceil(1.2);
    const FLOOR: f32 = corvid_float::floor(1.8);
    const ROUND: f32 = corvid_float::round(1.5);
    const TRUNC: f32 = corvid_float::trunc(-1.8);
    const ABS: f32 = corvid_float::abs(-3.0);
    const SIGN: f32 = corvid_float::copysign(3.0, -1.0);
    const CLAMP: f32 = corvid_float::clamp(9.0, 0.0, 1.0);
    const CLAMP_FINITE: f32 = corvid_float::clamp_finite(f32::INFINITY, 0.0, 1.0);
    const DEMOTE: f32 = corvid_float::demote(0.5);

    assert!((SQRT - 2.0f32.sqrt()).abs() < 1e-6);
    assert!((SIN - 1.0f32.sin()).abs() < 1e-6);
    assert!((COS - 1.0f32.cos()).abs() < 1e-6);
    assert!((TAN - 0.5f32.tan()).abs() < 1e-6);
    assert_eq!(RECIP, 0.25);
    assert!((HYPOT - 5.0).abs() < 1e-6);
    assert_eq!(POWI, 1024.0);
    assert_eq!(CEIL, 2.0);
    assert_eq!(FLOOR, 1.0);
    assert_eq!(ROUND, 2.0);
    assert_eq!(TRUNC, -1.0);
    assert_eq!(ABS, 3.0);
    assert_eq!(SIGN, -3.0);
    assert_eq!(CLAMP, 1.0);
    assert_eq!(CLAMP_FINITE, 0.0);
    assert_eq!(DEMOTE, 0.5);
}

/// The same witness for the wide half, which is a second set of definitions
/// rather than the same ones generically — so it can regress on its own and has
/// to be checked on its own.
///
/// A separate test rather than a second block in the one above only because
/// `clippy::items_after_statements` is on: a `const` is an item, and an item
/// after an assertion is a warning the workspace turns into an error.
/// `clamp_finite` and `demote` are absent here because `wide` does not have
/// them — the first is the audio mixer's and the mixer works in `f32`, and the
/// second only ever narrows in the one direction.
#[test]
fn every_wide_function_is_const() {
    const WIDE_SQRT: f64 = corvid_float::wide::sqrt(16.0);
    const WIDE_SIN: f64 = corvid_float::wide::sin(1.0);
    const WIDE_COS: f64 = corvid_float::wide::cos(1.0);
    const WIDE_TAN: f64 = corvid_float::wide::tan(0.5);
    const WIDE_RECIP: f64 = corvid_float::wide::recip(4.0);
    const WIDE_HYPOT: f64 = corvid_float::wide::hypot(3.0, 4.0);
    const WIDE_POWI: f64 = corvid_float::wide::powi(2.0, 10);
    const WIDE_CEIL: f64 = corvid_float::wide::ceil(1.2);
    const WIDE_FLOOR: f64 = corvid_float::wide::floor(1.8);
    const WIDE_ROUND: f64 = corvid_float::wide::round(1.5);
    const WIDE_TRUNC: f64 = corvid_float::wide::trunc(-1.8);
    const WIDE_ABS: f64 = corvid_float::wide::abs(-3.0);
    const WIDE_SIGN: f64 = corvid_float::wide::copysign(3.0, -1.0);
    const WIDE_CLAMP: f64 = corvid_float::wide::clamp(9.0, 0.0, 1.0);

    assert!((WIDE_SQRT - 4.0).abs() < 1e-12);
    assert!((WIDE_SIN - 1.0f64.sin()).abs() < 1e-12);
    assert!((WIDE_COS - 1.0f64.cos()).abs() < 1e-12);
    assert!((WIDE_TAN - 0.5f64.tan()).abs() < 1e-12);
    assert_eq!(WIDE_RECIP, 0.25);
    assert!((WIDE_HYPOT - 5.0).abs() < 1e-12);
    assert_eq!(WIDE_POWI, 1024.0);
    assert_eq!(WIDE_CEIL, 2.0);
    assert_eq!(WIDE_FLOOR, 1.0);
    assert_eq!(WIDE_ROUND, 2.0);
    assert_eq!(WIDE_TRUNC, -1.0);
    assert_eq!(WIDE_ABS, 3.0);
    assert_eq!(WIDE_SIGN, -3.0);
    assert_eq!(WIDE_CLAMP, 1.0);
}

/// The other half of the `const` claim: the constants the crate re-exports so
/// that a caller reaching for a `PI` names one crate rather than two. They come
/// straight from `core`, but a re-export can be dropped or misspelled and the
/// only place that would show is a downstream build.
#[test]
fn the_re_exported_constants_are_the_core_ones() {
    assert_eq!(corvid_float::consts::PI, core::f32::consts::PI);
    assert_eq!(
        corvid_float::consts::FRAC_PI_2,
        core::f32::consts::FRAC_PI_2
    );
    assert_eq!(corvid_float::wide::consts::PI, core::f64::consts::PI);
}
