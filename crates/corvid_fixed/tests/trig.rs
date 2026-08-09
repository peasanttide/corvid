//! Verifies the trigonometry against `f64`, exhaustively where the domain
//! allows.
//!
//! [`Angle8`] and [`Angle16`] have 256 and 65536 possible inputs, so every
//! result is checked against a reference computed in `f64`. Errors are measured
//! in units of the output type's last bit, and the asserted limits are
//! regression bounds: they are what the implementation currently achieves, so
//! any loss of accuracy shows up as a failure rather than a silent drift.
//!
//! [`Angle32`] is out of `f64`'s reach -- its last bit is 4.7e-10, and forming
//! the argument in `f64` already costs a millionth of that. So it gets a sampled
//! sweep at one-bit tolerance to catch gross breakage, and a table of values
//! computed in 80-digit arithmetic to pin correct rounding where it is actually
//! in question.

#![allow(
    clippy::panic_in_result_fn,
    clippy::missing_panics_doc,
    clippy::float_cmp,
    reason = "tests assert; a panic is how a test reports failure"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::suboptimal_flops,
    reason = "these tests feed edge-case bit patterns through narrowing casts on purpose"
)]

mod common;

use common::{Rng, Worst};
use corvid_fixed::{Angle8, Angle16, Angle32, I24F8, Signed8, Signed16, Signed32};

/// The reference sine of a phase, as the output type's nearest bit pattern.
fn reference(phase: f64, turn: f64, scale: f64, quarter_offset: f64) -> i128 {
    let radians = (phase / turn + quarter_offset) * core::f64::consts::TAU;
    (radians.sin() * scale).round() as i128
}

#[test]
fn sin_and_cos_are_exhaustively_correctly_rounded_for_angle8() {
    let scale = f64::from(Signed8::MAX.to_bits());
    let mut sin = Worst::default();
    let mut cos = Worst::default();

    for bits in 0..=u8::MAX {
        let angle = Angle8::from_bits(bits);
        let phase = f64::from(bits);
        sin.observe(
            i128::from(bits),
            i128::from(angle.sin().to_bits()),
            reference(phase, 256.0, scale, 0.0),
        );
        cos.observe(
            i128::from(bits),
            i128::from(angle.cos().to_bits()),
            reference(phase, 256.0, scale, 0.25),
        );
    }

    assert_eq!(sin.checked, 256);
    sin.assert_within(0, "Angle8::sin");
    cos.assert_within(0, "Angle8::cos");
}

#[test]
fn sin_and_cos_are_exhaustively_correctly_rounded_for_angle16() {
    let scale = f64::from(Signed16::MAX.to_bits());
    let mut sin = Worst::default();
    let mut cos = Worst::default();

    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        let phase = f64::from(bits);
        sin.observe(
            i128::from(bits),
            i128::from(angle.sin().to_bits()),
            reference(phase, 65_536.0, scale, 0.0),
        );
        cos.observe(
            i128::from(bits),
            i128::from(angle.cos().to_bits()),
            reference(phase, 65_536.0, scale, 0.25),
        );
    }

    assert_eq!(sin.checked, 65_536);
    sin.assert_within(0, "Angle16::sin");
    cos.assert_within(0, "Angle16::cos");
}

#[test]
fn sin_and_cos_are_within_one_bit_for_angle32() {
    let scale = f64::from(Signed32::MAX.to_bits());
    let turn = 4_294_967_296.0;
    let mut sin = Worst::default();
    let mut cos = Worst::default();
    let mut rng = Rng::new(0x5eed_1234);

    // Every boundary the octant folding can land on, plus a wide sweep.
    let mut phases = vec![0_u32, 1, u32::MAX];
    for octant in 0..8_u32 {
        let base = octant << 29;
        phases.extend([base.wrapping_sub(1), base, base + 1]);
    }
    phases.extend((0..20_000).map(|_| rng.next_u32()));

    for phase in phases {
        let angle = Angle32::from_bits(phase);
        let reference_phase = f64::from(phase);
        sin.observe(
            i128::from(phase),
            i128::from(angle.sin().to_bits()),
            reference(reference_phase, turn, scale, 0.0),
        );
        cos.observe(
            i128::from(phase),
            i128::from(angle.cos().to_bits()),
            reference(reference_phase, turn, scale, 0.25),
        );
    }

    // The tolerance is one bit because the *reference* needs it, not the
    // implementation. Forming the argument as `phase / 2^32 * TAU` in f64 costs
    // about 2^-51 radians, which is 2^-20 of Signed32's last bit, so this
    // reference mis-rounds roughly one near-tie in a million and a different
    // libm would pick different ones.
    //
    // That leaves this test unable to say anything about correct rounding at
    // this width. What does:
    // `sin_and_cos_match_exact_arithmetic_for_angle32` below, whose expectations
    // come from 80-digit arithmetic rather than f64, and
    // `trig::tests::sin_snorm_is_exhaustively_correctly_rounded_for_angle32`
    // in the crate, which walks all 2^32 phases.
    sin.assert_within(1, "Angle32::sin");
    cos.assert_within(1, "Angle32::cos");
}

/// Phases whose sine and cosine were computed in 80-digit decimal arithmetic,
/// as `(phase, sin bits, cos bits)` for [`Signed32`].
///
/// Every octant boundary, a spread of ordinary phases, and -- the point of the
/// table -- fourteen deliberately hunted near-ties, phases whose scaled sine
/// lands within `3e-4` of a halfway case. Those are the only inputs where a
/// rounding can go wrong, and they are exactly the ones a sampled sweep is least
/// likely to visit. `2688335011` is the phase that caught the old seven-term
/// polynomial rounding the wrong way.
///
/// Generated offline by summing the Taylor series at 80 digits with pi from
/// Machin's formula, then cross-checked against a second reduction that folds no
/// octants at all.
const EXACT: &[(u32, i32, i32)] = &[
    (0, 0, 2_147_483_647),
    (1, 3, 2_147_483_647),
    (116_373_459, 363_834_522, 2_116_438_153),
    (417_518_579, 1_231_623_298, 1_759_201_543),
    (429_662_622, 1_262_680_812, 1_737_044_381),
    (525_229_530, 1_492_420_647, 1_544_139_445),
    (536_870_911, 1_518_500_247, 1_518_500_252),
    (536_870_912, 1_518_500_249, 1_518_500_249),
    (536_870_913, 1_518_500_252, 1_518_500_247),
    (820_497_106, 2_001_787_642, 777_516_719),
    (859_451_904, 2_042_822_917, 662_239_037),
    (1_073_741_823, 2_147_483_647, 3),
    (1_073_741_824, 2_147_483_647, 0),
    (1_073_741_825, 2_147_483_647, -3),
    (1_097_127_993, 2_146_226_993, -73_455_485),
    (1_273_427_776, 2_056_503_994, -618_447_523),
    (1_478_749_876, 1_781_446_680, -1_199_222_140),
    (1_539_898_300, 1_667_190_106, -1_353_574_219),
    (1_610_612_735, 1_518_500_252, -1_518_500_247),
    (1_610_612_736, 1_518_500_249, -1_518_500_249),
    (1_610_612_737, 1_518_500_247, -1_518_500_252),
    (1_872_394_159, 841_080_191, -1_975_922_601),
    (2_147_483_647, 3, -2_147_483_647),
    (2_147_483_648, 0, -2_147_483_647),
    (2_147_483_649, -3, -2_147_483_647),
    (2_452_229_818, -925_987_834, -1_937_584_204),
    (2_632_049_108, -1_397_976_791, -1_630_137_082),
    (2_675_342_405, -1_498_348_884, -1_538_387_674),
    (2_684_354_559, -1_518_500_247, -1_518_500_252),
    (2_684_354_560, -1_518_500_249, -1_518_500_249),
    (2_684_354_561, -1_518_500_252, -1_518_500_247),
    (2_688_335_011, -1_527_316_793, -1_509_632_216),
    (2_800_454_814, -1_753_322_177, -1_239_978_773),
    (2_965_342_609, -1_998_772_002, -785_236_589),
    (2_965_446_622, -1_998_891_462, -784_932_441),
    (3_177_840_169, -2_143_159_710, -136_207_458),
    (3_185_950_873, -2_144_624_953, -110_769_243),
    (3_221_225_471, -2_147_483_647, -3),
    (3_221_225_472, -2_147_483_647, 0),
    (3_221_225_473, -2_147_483_647, 3),
    (3_415_330_359, -2_061_484_795, 601_636_480),
    (3_614_262_064, -1_802_174_711, 1_167_840_882),
    (3_758_096_383, -1_518_500_252, 1_518_500_247),
    (3_758_096_384, -1_518_500_249, 1_518_500_249),
    (3_758_096_385, -1_518_500_247, 1_518_500_252),
    (4_047_736_481, -759_875_433, 2_008_550_557),
    (4_047_793_130, -759_708_977, 2_008_613_523),
    (4_049_116_830, -755_817_945, 2_010_080_906),
    (4_294_967_295, -3, 2_147_483_647),
];

#[test]
fn sin_and_cos_match_exact_arithmetic_for_angle32() {
    for &(phase, sine, cosine) in EXACT {
        let angle = Angle32::from_bits(phase);
        assert_eq!(
            angle.sin().to_bits(),
            sine,
            "sin at phase {phase} is not correctly rounded"
        );
        assert_eq!(
            angle.cos().to_bits(),
            cosine,
            "cos at phase {phase} is not correctly rounded"
        );
    }
    assert_eq!(EXACT.len(), 49, "the table should not shrink unnoticed");
}

#[test]
fn quarter_turns_are_exact() {
    assert_eq!(Angle16::ZERO.sin(), Signed16::ZERO);
    assert_eq!(Angle16::ZERO.cos(), Signed16::MAX);
    assert_eq!(Angle16::QUARTER_TURN.sin(), Signed16::MAX);
    assert_eq!(Angle16::QUARTER_TURN.cos(), Signed16::ZERO);
    assert_eq!(Angle16::HALF_TURN.sin(), Signed16::ZERO);
    assert_eq!(Angle16::HALF_TURN.cos(), Signed16::MIN);
    assert_eq!(Angle16::THREE_QUARTER_TURN.sin(), Signed16::MIN);
    assert_eq!(Angle16::THREE_QUARTER_TURN.cos(), Signed16::ZERO);

    assert_eq!(Angle8::QUARTER_TURN.sin(), Signed8::MAX);
    assert_eq!(Angle32::QUARTER_TURN.sin(), Signed32::MAX);
    assert_eq!(Angle32::HALF_TURN.cos(), Signed32::MIN);
}

#[test]
fn sine_is_odd_and_cosine_is_even() {
    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        let mirrored = -angle;
        assert_eq!(mirrored.sin(), -angle.sin(), "sin(-x) != -sin(x) at {bits}");
        assert_eq!(mirrored.cos(), angle.cos(), "cos(-x) != cos(x) at {bits}");
    }
}

#[test]
fn pythagorean_identity_holds() {
    // sin^2 + cos^2 = 1, to within the rounding of two squarings.
    for bits in 0..=u16::MAX {
        let (sin, cos) = Angle16::from_bits(bits).sin_cos();
        let sum = f64::from(sin.to_bits()).powi(2) + f64::from(cos.to_bits()).powi(2);
        let unit = f64::from(Signed16::MAX.to_bits()).powi(2);
        let error = (sum / unit - 1.0).abs();
        assert!(error < 1e-4, "sin^2 + cos^2 off by {error:e} at {bits}");
    }
}

#[test]
fn sin_cos_agrees_with_the_separate_calls() {
    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        assert_eq!(angle.sin_cos(), (angle.sin(), angle.cos()));
    }
}

#[test]
fn cosine_leads_sine_by_a_quarter_turn() {
    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        assert_eq!(angle.cos(), (angle + Angle16::QUARTER_TURN).sin());
    }
}

#[test]
fn fast_sine_is_within_its_documented_error() {
    let scale = f64::from(Signed16::MAX.to_bits());
    let mut worst = Worst::default();
    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        worst.observe(
            i128::from(bits),
            i128::from(angle.sin_fast().to_bits()),
            reference(f64::from(bits), 65_536.0, scale, 0.0),
        );
    }
    // 1.2e-3 of full scale, in units of Signed16's last bit. The worst over this
    // domain is 1.0965e-3, at bits 63483; over the full 2^32 phases it is
    // 1.1111e-3, which the exhaustive test below pins. That is why the documented
    // bound is 1.2e-3 and not the 1.1e-3 the 16-bit sweep alone would suggest.
    let limit = (1.2e-3 * scale).ceil() as i128;
    worst.assert_within(limit, "Angle16::sin_fast");

    // The same bound stated the way the docs state it, so a doc that drifts
    // away from the implementation fails here rather than misleading a caller
    // budgeting error.
    let mut worst_absolute = 0.0_f64;
    for bits in 0..=u16::MAX {
        let expected = (f64::from(bits) / 65_536.0 * core::f64::consts::TAU).sin();
        let actual = Angle16::from_bits(bits).sin_fast().to_f64();
        worst_absolute = worst_absolute.max((actual - expected).abs());
    }
    assert!(
        worst_absolute <= 1.2e-3,
        "sin_fast worst absolute error {worst_absolute:e} exceeds the documented 1.2e-3"
    );
    assert!(
        worst_absolute > 1.05e-3,
        "sin_fast improved to {worst_absolute:e}; tighten the documented bound"
    );

    // At 8-bit output the approximation is already exact to the last bit.
    let scale8 = f64::from(Signed8::MAX.to_bits());
    let mut worst8 = Worst::default();
    for bits in 0..=u8::MAX {
        worst8.observe(
            i128::from(bits),
            i128::from(Angle8::from_bits(bits).sin_fast().to_bits()),
            reference(f64::from(bits), 256.0, scale8, 0.0),
        );
    }
    worst8.assert_within(1, "Angle8::sin_fast");
}

#[test]
fn fast_trigonometry_is_exact_at_the_quarter_turns() {
    assert_eq!(Angle16::ZERO.sin_fast(), Signed16::ZERO);
    assert_eq!(Angle16::QUARTER_TURN.sin_fast(), Signed16::MAX);
    assert_eq!(Angle16::HALF_TURN.sin_fast(), Signed16::ZERO);
    assert_eq!(Angle16::THREE_QUARTER_TURN.sin_fast(), Signed16::MIN);

    assert_eq!(Angle16::ZERO.cos_fast(), Signed16::MAX);
    assert_eq!(Angle16::QUARTER_TURN.cos_fast(), Signed16::ZERO);
    assert_eq!(Angle16::HALF_TURN.cos_fast(), Signed16::MIN);
}

#[test]
fn fast_sine_is_exactly_odd_and_fast_cosine_exactly_even() {
    // Not "within a bit of odd" -- exactly odd, because the phase is folded about
    // the peak rather than shifted one-sidedly. Worth pinning: the property is
    // free from that fold and would be silently lost by a refactor that reached
    // for the cheaper-looking arithmetic.
    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        let mirror = Angle16::from_bits(bits.wrapping_neg());
        assert_eq!(
            mirror.sin_fast().to_bits(),
            -angle.sin_fast().to_bits(),
            "sin_fast is not odd at {bits}"
        );
        assert_eq!(
            mirror.cos_fast().to_bits(),
            angle.cos_fast().to_bits(),
            "cos_fast is not even at {bits}"
        );
    }

    // Negating the result above is only sound because the scale never reaches
    // the denormal bit pattern that a signed-normalized type reserves.
    for bits in 0..=u16::MAX {
        assert_ne!(
            Angle16::from_bits(bits).sin_fast().to_bits(),
            i16::MIN,
            "sin_fast emitted a denormal at {bits}"
        );
    }
}

#[test]
fn fast_atan2_is_exact_on_the_axes() {
    assert_eq!(Angle16::atan2_fast(0, 1), Angle16::ZERO);
    assert_eq!(Angle16::atan2_fast(1, 0), Angle16::QUARTER_TURN);
    assert_eq!(Angle16::atan2_fast(0, -1), Angle16::HALF_TURN);
    assert_eq!(Angle16::atan2_fast(-1, 0), Angle16::THREE_QUARTER_TURN);
    assert_eq!(Angle16::atan2_fast(0, 0), Angle16::ZERO);

    // The half turn is the one place the Q30 accumulator reaches a value that
    // only survives the shift to a 32-bit phase by wrapping through `u32`.
    assert_eq!(Angle32::atan2_fast(0, -1), Angle32::HALF_TURN);
    assert_eq!(Angle32::atan2_fast(0, i32::MIN), Angle32::HALF_TURN);
}

#[test]
fn fast_atan2_survives_extreme_coordinates() {
    // The magnitudes that make a 32-bit intermediate overflow if the shifts are
    // wrong. Run under `cargo test` (debug), where overflow checks are on, this
    // is a proof rather than a smoke test.
    let extremes = [i32::MIN, i32::MIN + 1, -1, 0, 1, i32::MAX - 1, i32::MAX];
    for y in extremes {
        for x in extremes {
            let fast = Angle16::atan2_fast(y, x);
            if y != 0 || x != 0 {
                let exact = Angle16::atan2(i64::from(y), i64::from(x));
                let error = exact.abs_diff(fast).to_bits();
                assert!(error <= 46, "atan2_fast({y}, {x}) off by {error} bits");
            }
        }
    }

    // Walk the ratio domain end to end. With a divisor of exactly 2^15 the
    // normalization is a no-op and the quotient is the numerator, so this drives
    // the internal ratio through every value it can take -- and with it the
    // `r * (1 - r)` wedge, whose product with the correction weight is the
    // widest intermediate in the function.
    for y in 0..=32_768 {
        let fast = Angle32::atan2_fast(y, 32_768);
        let exact = Angle32::atan2(i64::from(y), 32_768);
        let limit = (4.4e-3 / core::f64::consts::TAU * 4_294_967_296.0).ceil() as u32;
        let error = exact.abs_diff(fast).to_bits();
        assert!(error <= limit, "atan2_fast({y}, 32768) off by {error} bits");
    }

    let mut rng = Rng::new(0x5f37_1e9d_c084_2b16);
    for _ in 0..200_000 {
        let y = rng.next_u64() as i32;
        let x = rng.next_u64() as i32;
        let fast = Angle32::atan2_fast(y, x);
        if y == 0 && x == 0 {
            continue;
        }
        let exact = Angle32::atan2(i64::from(y), i64::from(x));
        // 4.4e-3 radians in Angle32 bits.
        let limit = (4.4e-3 / core::f64::consts::TAU * 4_294_967_296.0).ceil() as u32;
        let error = exact.abs_diff(fast).to_bits();
        assert!(error <= limit, "atan2_fast({y}, {x}) off by {error} bits");
    }
}

/// The worst case over every phase there is, rather than a sample of them.
///
/// The bound the `_fast` docs quote comes from this sweep. Running it in debug,
/// where overflow checks are on, doubles as the proof that no `i32` intermediate
/// in the 32-bit kernel overflows anywhere in the domain.
///
/// Ignored because it walks all 2^32 phases. Run it with:
///
/// ```sh
/// cargo test -p corvid_fixed --release exhaustively_within -- --ignored
/// ```
#[test]
#[ignore = "walks all 2^32 phases; run explicitly"]
fn fast_sine_is_exhaustively_within_its_bound_for_angle32() {
    let threads = std::thread::available_parallelism().map_or(4, core::num::NonZero::get);
    let span = (1_u64 << 32) / threads as u64;

    let worst = std::thread::scope(|scope| {
        // The `collect` is what makes this parallel: fusing it into the fold
        // below would spawn each thread and immediately join it, walking the
        // domain one slice at a time.
        #[allow(
            clippy::needless_collect,
            reason = "the collect forces every thread to spawn before any is joined"
        )]
        let handles: Vec<_> = (0..threads)
            .map(|slot| {
                scope.spawn(move || {
                    let start = slot as u64 * span;
                    let end = if slot + 1 == threads {
                        1_u64 << 32
                    } else {
                        start + span
                    };
                    let mut worst = 0.0_f64;
                    for phase in start..end {
                        let phase = phase as u32;
                        let expected =
                            (f64::from(phase) / 4_294_967_296.0 * core::f64::consts::TAU).sin();
                        let actual = Angle32::from_bits(phase).sin_fast().to_f64();
                        worst = worst.max((actual - expected).abs());
                    }
                    worst
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or(f64::NAN))
            .fold(0.0_f64, f64::max)
    });

    assert!(
        worst <= 1.2e-3,
        "sin_fast worst absolute error {worst:e} exceeds the documented 1.2e-3"
    );
    assert!(
        worst > 1.05e-3,
        "sin_fast improved to {worst:e}; tighten the documented bound"
    );
}

#[test]
fn tangent_matches_the_reference_away_from_the_poles() {
    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        let expected = angle.to_radians().tan();
        if !(-1000.0..=1000.0).contains(&expected) {
            continue;
        }
        let actual = angle.tan().to_f64();
        // I24F8 resolves to 1/256; the tangent's slope multiplies that up.
        let tolerance = 1.0_f64.max(expected.abs()) * 0.01 + 0.004;
        assert!(
            (actual - expected).abs() < tolerance,
            "tan at {bits}: {actual} vs {expected}"
        );
    }
}

#[test]
fn tangent_saturates_at_the_poles() {
    assert_eq!(Angle16::QUARTER_TURN.tan(), I24F8::MAX);
    assert_eq!(Angle16::THREE_QUARTER_TURN.tan(), I24F8::MIN);
    assert_eq!(Angle16::ZERO.tan(), I24F8::ZERO);
    assert_eq!(Angle16::HALF_TURN.tan(), I24F8::ZERO);
    assert_eq!(Angle32::QUARTER_TURN.tan(), I24F8::MAX);
}

#[test]
fn tangent_is_the_ratio_of_sine_to_cosine() {
    // An eighth of a turn is where both are equal, so the tangent is one.
    let eighth = Angle16::from_bits(8192);
    assert_eq!(eighth.tan(), I24F8::ONE);
    assert_eq!((-Angle16::from_bits(8192)).tan(), -I24F8::ONE);
}

#[test]
fn atan2_inverts_sin_cos() {
    // Round-tripping an angle through its own sine and cosine must return it.
    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        let (sin, cos) = angle.sin_cos();
        let recovered = Angle16::atan2(i64::from(sin.to_bits()), i64::from(cos.to_bits()));
        let error = angle.abs_diff(recovered).to_bits();
        assert!(error <= 1, "atan2 round-trip off by {error} bits at {bits}");
    }
}

#[test]
fn atan2_matches_the_reference_over_a_grid() {
    let mut worst = Worst::default();
    for y in -64_i64..=64 {
        for x in -64_i64..=64 {
            if x == 0 && y == 0 {
                continue;
            }
            let expected = (y as f64).atan2(x as f64) / core::f64::consts::TAU;
            let expected_bits = (expected.rem_euclid(1.0) * 65_536.0).round() as i128 % 65_536;
            let actual = i128::from(Angle16::atan2(y, x).to_bits());
            // Compare on the circle: 0 and 65535 are one bit apart.
            let direct = (actual - expected_bits).abs();
            let wrapped = 65_536 - direct;
            worst.observe(i128::from(y * 1000 + x), direct.min(wrapped), 0);
        }
    }
    worst.assert_within(0, "Angle16::atan2 over a grid");
}

#[test]
fn atan2_handles_the_axes_and_the_origin() {
    assert_eq!(Angle16::atan2(0, 0), Angle16::ZERO);
    assert_eq!(Angle16::atan2(0, 5), Angle16::ZERO);
    assert_eq!(Angle16::atan2(5, 0), Angle16::QUARTER_TURN);
    assert_eq!(Angle16::atan2(0, -5), Angle16::HALF_TURN);
    assert_eq!(Angle16::atan2(-5, 0), Angle16::THREE_QUARTER_TURN);

    assert_eq!(Angle16::atan2(1, 1), Angle16::from_degrees(45.0));
    assert_eq!(Angle16::atan2(1, -1), Angle16::from_degrees(135.0));
    assert_eq!(Angle16::atan2(-1, -1), Angle16::from_degrees(225.0));
    assert_eq!(Angle16::atan2(-1, 1), Angle16::from_degrees(315.0));
}

#[test]
fn atan2_is_scale_invariant() {
    let base = Angle32::atan2(3, 7);
    for scale in [1_i64, 2, 17, 1024, 1_000_000, 1_000_000_000] {
        let scaled = Angle32::atan2(3 * scale, 7 * scale);
        let error = base.abs_diff(scaled).to_bits();
        assert!(error <= 2, "scale {scale} moved the angle by {error} bits");
    }
}

#[test]
fn atan2_survives_extreme_coordinates() {
    // No overflow, no panic, and the quadrant is still right.
    assert_eq!(
        Angle16::atan2(i64::MAX, i64::MAX),
        Angle16::from_degrees(45.0)
    );
    assert_eq!(
        Angle16::atan2(i64::MIN, i64::MIN),
        Angle16::from_degrees(225.0)
    );
    assert_eq!(Angle16::atan2(0, i64::MIN), Angle16::HALF_TURN);
    assert_eq!(Angle16::atan2(1, i64::MAX), Angle16::ZERO);
}

#[test]
fn fast_atan2_is_within_its_documented_error() {
    // 4.4e-3 radians, expressed in Angle16 bits.
    let limit = (4.4e-3 / core::f64::consts::TAU * 65_536.0).ceil() as u16;
    let mut worst = 0_u16;
    for y in -40_i32..=40 {
        for x in -40_i32..=40 {
            let exact = Angle16::atan2(i64::from(y), i64::from(x));
            let fast = Angle16::atan2_fast(y, x);
            worst = worst.max(exact.abs_diff(fast).to_bits());
        }
    }
    assert!(
        worst <= limit,
        "fast atan2 off by {worst} bits (limit {limit})"
    );
}

#[test]
fn trigonometry_is_available_in_const_context() {
    const HEADING: Angle16 = Angle16::from_degrees(120.0);
    const SIN: Signed16 = HEADING.sin();
    const COS: Signed16 = HEADING.cos();
    const TAN: I24F8 = HEADING.tan();
    const FAST: Signed16 = HEADING.sin_fast();
    const BACK: Angle16 = Angle16::atan2(1, -1);
    const BACK_FAST: Angle16 = Angle16::atan2_fast(1, -1);

    assert!((SIN.to_f64() - 0.866).abs() < 1e-3);
    assert!((COS.to_f64() + 0.5).abs() < 1e-3);
    assert!((TAN.to_f64() + 1.732).abs() < 1e-2);
    assert!((FAST.to_f64() - 0.866).abs() < 2e-3);
    assert_eq!(BACK, Angle16::from_degrees(135.0));
    assert!(BACK_FAST.abs_diff(BACK).to_bits() < 100);
}
