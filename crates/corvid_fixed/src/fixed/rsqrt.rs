//! The integer reciprocal square root shared by the fixed-point family.
//!
//! Every normalize in Corvid — a quaternion decode, a direction, a rotation
//! matrix row — asks for `1/sqrt(x)` and nothing else. Composing the two
//! operations the crate already has is the wrong answer twice over.
//!
//! **Accuracy.** `x.sqrt().recip()` rounds at the square root and again at the
//! reciprocal, and the intermediate is a value the caller never wanted: for
//! small `x`, `sqrt(x)` lands near the bottom of the type's resolution and the
//! reciprocal then amplifies that quantization error. [`rsqrt_bits`] rounds
//! once, from a full-width intermediate, like every other operation here.
//!
//! **Speed.** `sqrt` is an `isqrt` loop and `recip` adds a wide divide. This
//! uses neither: the estimate is seeded from `leading_zeros` and refined by
//! Newton–Raphson, whose steps are two multiplies and a shift each.
//!
//! Newton converges quadratically, so the last step is followed by one exact
//! integer comparison that picks between the two neighbouring results. That
//! lands the answer **correctly rounded**, not merely close — the standard the
//! crate already holds `sqrt` and `mul` to.

/// `1.0` in the wide Q62 working scale.
const ONE: u64 = 1 << 62;

/// `1.0` in the narrow Q30 working scale.
const ONE_NARROW: u64 = 1 << 30;

/// `(1 − 1/√2) · 2^30`: minus the slope of the linear seed over `[1, 2)`.
///
/// Written as exact integer arithmetic rather than a transcribed decimal —
/// `2^30/√2` is `√(2^59)`, which [`u64::isqrt`] evaluates at compile time.
const SLOPE_HIGH: u64 = ONE_NARROW - (1u64 << 59).isqrt();

/// The seed's value at `n = 0`, extrapolated from the `[1, 2)` fit.
const INTERCEPT_HIGH: u64 = ONE_NARROW + SLOPE_HIGH;

/// `2(√2 − 1) · 2^30`: minus the slope of the linear seed over `[0.5, 1)`.
const SLOPE_LOW: u64 = 2 * ((1u64 << 61).isqrt() - ONE_NARROW);

/// The seed's value at `n = 0`, extrapolated from the `[0.5, 1)` fit.
const INTERCEPT_LOW: u64 = ONE_NARROW + SLOPE_LOW;

/// Newton steps taken at Q30, where every intermediate fits `u64`.
///
/// The linear seeds hold `|relative error|` under 4.5% across each half, and a
/// step takes `e` to about `1.5 e²`: 4.5e-2 → 3.1e-3 → 1.4e-5 → 2.9e-10. Three
/// steps therefore reach Q30's own last bit, which is as far as narrow
/// arithmetic can carry the estimate.
const NARROW_STEPS: u32 = 3;

/// Bits of answer the narrow phase can be trusted for.
///
/// Q30 arithmetic cannot carry a relative error below about `2^-29`, so an
/// `m`-bit answer leaves an absolute error near `2^(m-29)` last bits. Stopping
/// at 27 keeps that under an eighth of a step, which is all the correction
/// below needs. `I0F8`, `I8F8` and `I24F8` produce 12-bit answers and `I16F16`
/// and `I48F16` produce 24-bit ones, so all five stop here; only `I2F30`, whose
/// answer reaches 45 bits, goes on to the Q62 step.
const NARROW_BITS: u32 = 27;

// One further step at Q62 squares that error again — 2.9e-10 becomes 1.3e-19,
// comfortably below the last bit of any result this family can hold. Splitting
// the iteration this way is what keeps `rsqrt` cheaper than the `sqrt().recip()`
// it replaces: only the final step pays for 128-bit multiplies.

/// `round(2^(3F/2) / sqrt(x))`, the bit pattern of `1/sqrt(x)` at `frac`
/// fractional bits.
///
/// `x` is the input's bit pattern and must be positive; callers handle zero and
/// negatives, which saturate. The result can exceed the caller's storage type —
/// `I2F30::rsqrt(DELTA)` alone reaches `2^45` — so callers saturate it. It
/// always fits `u64`, because `x >= 1` bounds the answer by `2^(3F/2)` and this
/// family's widest `frac` is 30.
///
/// # Panics
///
/// `frac` must be **even** and at most 30. The rescale in step 5 divides
/// `3 * frac + shift_up` by two and the `& !1` in step 1 keeps `shift_up` even,
/// so an odd `frac` loses half a bit there and every answer comes back a factor
/// of `√2` out — a 41% error, silently. Both conditions are checked here rather
/// than left to the doc comment, because `define_fixed_point!` will happily
/// instantiate any `frac` and the next type to be added is the one that would
/// find out.
///
/// The assertion is over a literal at every call site, so it folds away.
#[inline]
pub(super) const fn rsqrt_bits(x: u64, frac: u32) -> u64 {
    assert!(
        frac.is_multiple_of(2) && frac <= 30,
        "rsqrt needs an even `frac` of at most 30"
    );
    // 1. Normalize. `x` came from a positive value of a signed type, so its bit
    //    length is at most 63 and the shift is never negative. Rounding the
    //    shift down to even keeps `3 * frac + shift` even, which is what makes
    //    the rescale in step 5 an exact power of two rather than a half power.
    let bit_length = 64 - x.leading_zeros();
    let shift_up = (63 - bit_length) & !1;
    let n = x << shift_up;

    // `n` now lies in `[2^61, 2^63)`, so the value it denotes at Q62 is in
    // `[0.5, 2)` — one binade either side of 1, which is what the two-piece
    // seed below is fitted to.

    // 2. Seed. A straight line through the endpoints of each half, at Q30.
    let narrow = n >> 32;
    let mut q = if narrow >= ONE_NARROW {
        INTERCEPT_HIGH - ((narrow * SLOPE_HIGH) >> 30)
    } else {
        INTERCEPT_LOW - ((narrow * SLOPE_LOW) >> 30)
    };

    // 3. Newton at Q30: q <- q (3 - n q^2) / 2. `q` stays under `1.5 * 2^30`
    //    and `n` under `2^31`, so every product here fits `u64`.
    let mut step = 0;
    while step < NARROW_STEPS {
        let squared = (q * q) >> 30;
        let scaled = (narrow * squared) >> 30;
        // `n q^2` approaches 1 from either side and never reaches 1.1, so this
        // subtraction cannot underflow.
        q = (q * (3 * ONE_NARROW - scaled)) >> 31;
        step += 1;
    }

    // 4. One more step at Q62, which is where the 128-bit multiplies live —
    //    but only for the types that need it. The narrow phase carries about
    //    30 bits, and an answer of `3 * frac / 2` bits needs a couple more than
    //    that. `frac` is a literal at every call site, so this branch folds
    //    away entirely.
    let mut q = q << 32;
    if 3 * frac / 2 >= NARROW_BITS {
        let squared = mul_q62(q, q);
        let scaled = mul_q62(n, squared);
        q = (((q as u128) * ((3 * ONE - scaled) as u128)) >> 63) as u64;
    }

    // Newton converges from below — the error term is `-1.5 e^2` — so `q` is a
    // slight underestimate either way, which is what the floor in step 5 and
    // the correction in step 6 are written for.

    // 5. Rescale. With `n = x * 2^shift_up` and `q = 2^62 / sqrt(n)`, the
    //    answer is `q * 2^((3 * frac + shift_up - 186) / 2)`. That exponent is
    //    always negative — `shift_up <= 62` and `3 * frac <= 90` — so this is
    //    only ever a right shift, by 17 to 81 places. The truncating shift
    //    floors, which together with Newton's underestimate leaves
    //    `candidate` at most one below the correctly rounded answer.
    let down = (186 - 3 * frac - shift_up) / 2;
    let candidate = if down >= u64::BITS { 0 } else { q >> down };

    // 6. Correct exactly. Newton plus the truncating shift leaves `candidate`
    //    either the correctly rounded answer or one below it. `candidate` is
    //    the answer exactly when `candidate + 0.5` exceeds the true value,
    //    which rearranges into a comparison of integers:
    //
    //        (2 * candidate + 1)^2 * x  >  2^(3 * frac + 2)
    //
    //    Both sides are near `2^(3 * frac + 2)` by construction — the product's
    //    two factors move inversely — so `u128` holds them with room to spare,
    //    and equality means an exact tie, which rounds away from zero.
    let midpoint = 2 * candidate as u128 + 1;
    if midpoint * midpoint * x as u128 > 1u128 << (3 * frac + 2) {
        candidate
    } else {
        candidate + 1
    }
}

/// The Q62 product of two Q62 values, truncated.
///
/// Both operands stay `u64`, so this is a 64×64-to-128 multiply and a shift
/// rather than `u128` arithmetic.
const fn mul_q62(a: u64, b: u64) -> u64 {
    (((a as u128) * (b as u128)) >> 62) as u64
}

/// `1.0` in the Q15 working scale of the approximate tier.
const ONE_FAST: u32 = 1 << 15;

/// `(1 − 1/√2) · 2^15`: minus the slope of the fast seed over `[1, 2)`.
///
/// Exact integer arithmetic for the same reason [`SLOPE_HIGH`] is: `2^15/√2` is
/// `√(2^29)`, which [`u32::isqrt`] evaluates at compile time.
const SLOPE_HIGH_FAST: u32 = ONE_FAST - (1u32 << 29).isqrt();

/// The fast seed's value at `n = 0`, extrapolated from the `[1, 2)` fit.
const INTERCEPT_HIGH_FAST: u32 = ONE_FAST + SLOPE_HIGH_FAST;

/// `2(√2 − 1) · 2^15`: minus the slope of the fast seed over `[0.5, 1)`.
const SLOPE_LOW_FAST: u32 = 2 * ((1u32 << 31).isqrt() - ONE_FAST);

/// The fast seed's value at `n = 0`, extrapolated from the `[0.5, 1)` fit.
const INTERCEPT_LOW_FAST: u32 = ONE_FAST + SLOPE_LOW_FAST;

/// `round(2^(3F/2) / sqrt(x))` again, approximately, in 32 bits throughout.
///
/// The counterpart of [`rsqrt_bits`] for the crate's approximate tier, and
/// 32-bit clean for the same reason `trig::sin_fast_q30` is: no intermediate
/// leaves `u32`/`i32`, no product needs a widening multiply, and every
/// operation has a `WGSL` equivalent, so the routine transcribes straight into
/// a shader.
///
/// `x` is the input's bit pattern and must be positive; callers handle zero and
/// negatives. The result is capped at `i32::MAX`, which is at or above every
/// [`MAX`](crate::I2F30::MAX) in the family, so a caller's own saturation sees
/// the same answer it would have from [`rsqrt_bits`].
///
/// # Why the accuracy stops where it does
///
/// The relative error is `3.2e-5`, or a shade under 15 significant bits, and
/// **that is the ceiling for 32-bit arithmetic**, not a choice of step count.
/// Newton refines `q` by the residual `1 − n q²`, and a residual is only as good
/// as the product inside it: `n` and `q²` each have to fit an operand, their
/// product has to fit a register, so `n` gets about 15 bits and `q²` about 15,
/// and the residual inherits both truncations. More steps cannot recover bits
/// the multiply never carried. [`rsqrt_bits`] escapes this by widening — it
/// spends a 64×64-to-128 multiply on exactly this product — which is the thing
/// this tier exists to avoid.
///
/// Two steps are therefore enough. The seed holds `|relative error|` under 4.6%,
/// a step takes `e` to about `1.5 e²`, and 4.6e-2 → 3.2e-3 → 1.5e-5 lands under
/// the arithmetic floor with the second step. A third would refine nothing.
///
/// # Panics
///
/// `frac` must be **even** and at most 30, for the reason [`rsqrt_bits`] gives.
/// The assertion is over a literal at every call site, so it folds away.
#[inline]
pub(super) const fn rsqrt_fast_bits(x: u32, frac: u32) -> u32 {
    assert!(
        frac.is_multiple_of(2) && frac <= 30,
        "rsqrt needs an even `frac` of at most 30"
    );
    // 1. Normalize, as `rsqrt_bits` does but one word narrower. `x` came from a
    //    positive value of a signed type no wider than `i32`, so its bit length
    //    is at most 31 and the shift is never negative. Rounding the shift down
    //    to even keeps `3 * frac + shift` even, which is what makes the rescale
    //    in step 5 a whole power of two rather than a half power.
    let bit_length = 32 - x.leading_zeros();
    let shift_up = (31 - bit_length) & !1;
    let n = x << shift_up;

    // `n` now lies in `[2^29, 2^31)`, so the value it denotes at Q30 is in
    // `[0.5, 2)` — the same pair of binades the exact seed is fitted to.

    // 2. Narrow to Q15, which is where the residual's two factors have to meet:
    //    `n q^2` is near 1, so a Q15 `n` against a Q15 `q^2` lands the product
    //    near `2^30` and inside a register. Rounding rather than truncating
    //    costs nothing and halves the error this step contributes, which — see
    //    above — is half of the error budget for the whole routine.
    let nh = (n + (1 << 14)) >> 15;

    // 3. Seed. The same two-piece chord as the exact path, refitted to Q15.
    let q = if nh >= ONE_FAST {
        INTERCEPT_HIGH_FAST - ((nh * SLOPE_HIGH_FAST) >> 15)
    } else {
        INTERCEPT_LOW_FAST - ((nh * SLOPE_LOW_FAST) >> 15)
    };

    // 4. Two Newton steps, written as `q <- q + q (1 - n q^2) / 2` rather than
    //    `q <- q (3 - n q^2) / 2`. The two are the same arithmetic, but the
    //    residual form keeps the small quantity small: `1 - n q^2` shrinks
    //    quadratically, so the second step can hold it at a far finer scale
    //    than `3 - n q^2` — whose leading `2` would spend the register on bits
    //    that carry no information — and still keep `q * residual` in range.
    //
    //    The chord lies above `1/√n`, so the seed overestimates and the first
    //    residual is negative; every later one is positive, because Newton
    //    approaches from below. Both are handled by the signed shifts.
    let coarse = residual_q30(nh, q);
    // The seed's residual reaches 9% of 1, so it comes down to Q15 to multiply.
    // `q * (coarse >> 15)` is at most `46341 * 3021`.
    let q = (q as i32 + ((q as i32 * (coarse >> 15)) >> 16)) as u32;

    let fine = residual_q30(nh, q) >> 8;

    // 5. Emit at Q30 instead of folding back into Q15. `q` is exact as an
    //    integer but only carries 15 bits of the answer; the correction below
    //    is the next 15, and placing it at Q30 is what makes the residual form
    //    worth writing. `q << 15` is at most `2^30.5` and the correction at
    //    most `46341 * 26172 >> 8`, so the sum stays inside `i32`.
    let q30 = ((q << 15) as i32) + ((q as i32 * fine) >> 8);

    // 6. Rescale. Exactly the exponent `rsqrt_bits` derives, one word narrower:
    //    with `n = x * 2^shift_up` and `q30 = 2^30 / sqrt(n)`, the answer is
    //    `q30 * 2^((3 * frac + shift_up - 90) / 2)`. Unlike the wide path that
    //    exponent can be positive, because a Q30 answer can reach `2^45` while
    //    a `u32` stops at `2^32` — so this saturates where that one shifted.
    //    `frac` is a literal at every call site, so only one arm survives.
    let shift = (90 - 3 * frac as i32 - shift_up as i32) / 2;
    if shift < 0 {
        // Reachable only at `frac = 30`. `q30` is at least `2^29.5`, so two
        // doublings always pass the cap and one usually does.
        if shift < -1 || q30 > i32::MAX >> 1 {
            i32::MAX as u32
        } else {
            (q30 << 1) as u32
        }
    } else if shift >= u32::BITS as i32 {
        // The true answer is under half a step, so its nearest value is zero.
        0
    } else if shift == 0 {
        q30 as u32
    } else {
        // `q30 + 2^(shift-1)` peaks at `2^30.5 + 2^30`, which is why the
        // rounding happens in `u32` rather than `i32`.
        ((q30 as u32) + (1 << (shift - 1))) >> shift
    }
}

/// `(1 − n q²) · 2^30`, the Newton residual, from a Q15 `n` and a Q15 `q`.
///
/// Both products are bounded by what the iteration guarantees rather than by
/// the widths of their factors: `q` and `n` move inversely, so `q * q` reaches
/// `2^31` only where `n` is smallest, and `n q^2` stays within a tenth of 1
/// however extreme either one is. The square is held in `u32` because that
/// first bound is `2^31` exactly — the boundary `i32` cannot cross.
#[inline]
const fn residual_q30(nh: u32, q: u32) -> i32 {
    let squared = (q * q + (1 << 14)) >> 15;
    (1 << 30) - (nh * squared) as i32
}
