//! Error statistics for the 32-bit tier, against an `f64` reference.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::print_stdout,
    reason = "the measured figures are the point; run with --nocapture to read them"
)]
#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::suboptimal_flops,
    clippy::items_after_statements,
    reason = "tests reach into raw bit patterns on purpose, and their f64 references are written as plain arithmetic so they stay independent of the implementation"
)]

mod common;

use common::Rng;
use corvid_fixed::I2F30;
use corvid_rotation::{Basis, Rotation, Versor};

/// The stated budget for this tier.
///
/// Measured max is 0.1856°, so this leaves 7% of headroom. If the fixed-point
/// decode's rounding eats into it, the fix is widening the decode's
/// intermediate precision — **not** loosening this number. 1/5° is the budget,
/// and a test that moves to accommodate the implementation has stopped testing
/// anything.
const BUDGET_DEGREES: f64 = 0.2;

#[test]
fn round_trip_error_stays_inside_one_fifth_of_a_degree() {
    let mut rng = Rng::new(0x3200_0001);
    let mut worst = 0.0f64;
    let mut total = 0.0f64;
    const SAMPLES: u32 = 200_000;

    for _ in 0..SAMPLES {
        let reference = common::random_unit_quaternion_f64(&mut rng);
        let packed = Rotation::from_versor(common::versor_from_f64(reference));
        let decoded = common::to_f64_quaternion(packed.to_versor());
        let error = common::angle_degrees(reference, decoded);
        worst = worst.max(error);
        total += error;
    }

    let mean = total / f64::from(SAMPLES);
    println!("Rotation: mean {mean:.4} deg, max {worst:.4} deg over {SAMPLES} samples");
    assert!(worst < BUDGET_DEGREES, "max error {worst} degrees");
    // The measured figure is 0.0784 degrees, with room for the fixed-point
    // decode's own rounding on top.
    assert!(mean < 0.09, "mean error {mean} degrees");
}

#[test]
fn every_bit_pattern_decodes_to_a_unit_quaternion() {
    let mut rng = Rng::new(0x3200_0002);
    for _ in 0..200_000 {
        let q = Rotation::from_bits(rng.next_u32()).to_versor();
        let [x, y, z, w] = common::to_f64_quaternion(q);
        let norm = x * x + y * y + z * z + w * w;
        assert!((norm - 1.0).abs() < 1e-6, "{q:?} has squared norm {norm}");
    }

    // Including the patterns no encoder emits.
    for bits in [0, u32::MAX, 1, 1 << 30, 3 << 30] {
        let q = Rotation::from_bits(bits).to_versor();
        let [x, y, z, w] = common::to_f64_quaternion(q);
        let norm = x * x + y * y + z * z + w * w;
        assert!(
            (norm - 1.0).abs() < 1e-6,
            "bits {bits:#x} has squared norm {norm}"
        );
    }
}

#[test]
fn all_four_charts_are_reachable() {
    let mut rng = Rng::new(0x3200_0003);
    let mut seen = [false; 4];
    for _ in 0..10_000 {
        let reference = common::random_unit_quaternion_f64(&mut rng);
        let packed = Rotation::from_versor(common::versor_from_f64(reference));
        seen[(packed.to_bits() >> 30) as usize] = true;
    }
    assert_eq!(seen, [true; 4]);
}

#[test]
fn repacking_is_stable_and_bounded() {
    // `pack ∘ unpack` is the identity on almost every pattern, and where it is
    // not, it moves the rotation by less than the codec's own quantum.
    //
    // The exception is a chart tie: when two quaternion components are equal in
    // magnitude, a Gibbs field lands on exactly ±1 and *either* component is a
    // valid chart. Re-encoding then picks the lower-indexed one and re-
    // quantizes in the new chart. Both encodings name the same rotation to
    // within a fraction of a quantum, so this is a property of the codec rather
    // than a defect — but it is a property worth stating, not one to discover
    // in production.
    let mut rng = Rng::new(0x3200_0004);
    const SAMPLES: u32 = 100_000;
    let mut moved = 0u32;
    let mut worst = 0.0f64;

    for _ in 0..SAMPLES {
        let once = Rotation::from_versor(common::random_versor(&mut rng));
        let twice = Rotation::from_versor(once.to_versor());
        // Which is exactly what `is_canonical` asks, so the share counted below
        // is the share of encoder output that is not canonical — the figure the
        // docs quote against the 0.58% for arbitrary patterns.
        assert_eq!(once.is_canonical(), twice.to_bits() == once.to_bits());
        if twice.to_bits() != once.to_bits() {
            moved += 1;
            worst = worst.max(once.to_versor().angle_to(twice.to_versor()).to_degrees());
        }
    }

    let share = f64::from(moved) / f64::from(SAMPLES);
    println!(
        "Rotation repack: {moved} of {SAMPLES} patterns moved ({share:.5}), worst {worst:.4} deg"
    );
    // Chart ties are a measure-zero set; quantization makes them merely rare.
    // Banded rather than bounded above, because 0.065% is the figure the type
    // docs and the README quote for the encoder's own output — the number that
    // says the guarantee is "much smaller" and not "zero". An upper bound alone
    // would let a codec change drive it to zero and quietly make the prose
    // understate the type.
    assert!(
        (0.0004..0.0010).contains(&share),
        "{moved} of {SAMPLES} encoded patterns changed bits, a share of {share}"
    );
    // And the ones that move stay well inside the tier's budget.
    assert!(
        worst < BUDGET_DEGREES,
        "repacking moved a rotation by {worst} degrees"
    );
}

#[test]
fn an_arbitrary_bit_pattern_names_a_stable_rotation() {
    // Every `u32` is a valid rotation, and decoding, re-encoding and decoding
    // again lands within the codec's quantum of where it started — including
    // the patterns no encoder emits.
    let mut rng = Rng::new(0x3200_0006);
    for _ in 0..100_000 {
        let bits = rng.next_u32();
        let once = Rotation::from_bits(bits).to_versor();
        let twice = Rotation::from_versor(once).to_versor();
        let moved = once.angle_to(twice).to_degrees();
        assert!(
            moved < BUDGET_DEGREES,
            "bits {bits:#x} moved by {moved} degrees"
        );
    }
}

#[test]
fn identity_is_exact() {
    assert_eq!(Rotation::IDENTITY.to_basis(), Basis::IDENTITY);
    assert_eq!(Rotation::from_versor(Versor::IDENTITY), Rotation::IDENTITY);
    assert_eq!(Rotation::from_basis(Basis::IDENTITY), Rotation::IDENTITY);
    assert_eq!(Rotation::default(), Rotation::IDENTITY);
}

#[test]
fn a_quantization_tie_rounds_away_from_zero_in_both_directions() {
    // `quantize` rounds half away from zero, the rule every other reduction in
    // the crate uses, so a rotation and its mirror image encode to fields that
    // are exact negations rather than landing one step apart. Only an exact tie
    // can tell that apart from rounding half up, and a random pose never lands
    // on one: this pair is built to.
    //
    // `w` is `1022` times `x`, and a field holds `round(x / w * 511)`, so
    // `x / w` is exactly half a field step. `1022 * UNIT` is just under `1.0` at
    // Q30, which makes the pair unit to within `from_xyzw`'s tolerance and
    // leaves `w` the chart.
    const UNIT: i32 = 1_050_627;
    let build = |x: i32| {
        Versor::from_xyzw(
            I2F30::from_bits(x),
            I2F30::ZERO,
            I2F30::ZERO,
            I2F30::from_bits(1022 * UNIT),
        )
        .expect("the constructed pair is unit to within the tolerance")
    };
    // The `x` field is the low one, since the chart is index 3.
    let field = |q| (Rotation::from_versor(q).to_bits() & 0x3FF) as i32 - 512;

    assert_eq!(Rotation::from_versor(build(UNIT)).to_bits() >> 30, 3);
    assert_eq!(field(build(UNIT)), 1, "a positive tie rounded toward zero");
    assert_eq!(
        field(build(-UNIT)),
        -1,
        "a negative tie rounded toward zero"
    );
}

#[test]
fn the_double_cover_packs_to_one_pattern() {
    let mut rng = Rng::new(0x3200_0005);
    for _ in 0..50_000 {
        let q = common::random_versor(&mut rng);
        assert_eq!(Rotation::from_versor(q), Rotation::from_versor(q.negate()));
    }
}

#[test]
fn about_six_patterns_in_a_thousand_are_not_canonical() {
    // The figure the type docs and the README quote, and the whole reason
    // `canonicalize` exists: `Eq`, `Hash` and `digest` are all on the raw
    // pattern here, so two peers that decoded one rotation from two patterns
    // disagree until someone folds them. Stating "a few" would let the number
    // drift; a codec change that moved it by a factor should have to say so.
    let mut rng = Rng::new(0x3200_0007);
    const SAMPLES: u32 = 1_000_000;
    let mut uncanonical = 0u32;

    for _ in 0..SAMPLES {
        let bits = rng.next_u32();
        let folded = Rotation::from_bits(bits).canonicalize();
        if folded.to_bits() != bits {
            uncanonical += 1;
        }
        // Folding twice has to be folding once, or a peer could not settle on
        // one pattern by folding.
        assert_eq!(
            folded.canonicalize(),
            folded,
            "bits {bits:#x} did not settle"
        );
    }

    let share = f64::from(uncanonical) / f64::from(SAMPLES);
    println!("Rotation: {uncanonical} of {SAMPLES} patterns are not canonical ({share:.5})");
    assert!(
        (0.005..0.007).contains(&share),
        "{uncanonical} of {SAMPLES} patterns were not canonical, a share of {share}"
    );
}

#[test]
fn the_codec_is_available_in_const_context() {
    const PACKED: Rotation = Rotation::from_versor(Versor::IDENTITY);
    const DECODED: Versor = PACKED.to_versor();
    const AS_MATRIX: Basis = PACKED.to_basis();

    assert_eq!(PACKED, Rotation::IDENTITY);
    assert_eq!(DECODED, Versor::IDENTITY);
    assert_eq!(AS_MATRIX, Basis::IDENTITY);
}
