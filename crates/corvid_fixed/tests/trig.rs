//! The exact trigonometry against `f64`, exhaustively where the domain
//! allows it.
//!
//! An 8- and a 16-bit angle have few enough phases to check every one, so the
//! claim of correct rounding is checked rather than sampled. A 32-bit angle is
//! sampled, and pinned exactly at the phases where integer arithmetic can say
//! what the answer is without a float in the way.

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
use corvid_fixed::{Angle8, Angle16, Angle32, Signed8, Signed16, Signed32};

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
