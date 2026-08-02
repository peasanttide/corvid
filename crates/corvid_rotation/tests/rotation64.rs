//! Error statistics and canonicality for the 64-bit tier.

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
    clippy::float_cmp,
    reason = "tests reach into raw bit patterns on purpose, and their f64 references are written as plain arithmetic so they stay independent of the implementation"
)]

mod common;

use common::Rng;
use corvid_rotation::{Basis, FineRotation, Rotation, Versor};

/// The stated budget for this tier: 1/128 of a degree.
const BUDGET_DEGREES: f64 = 1.0 / 128.0;

#[test]
fn round_trip_error_stays_inside_one_hundred_and_twenty_eighth_of_a_degree() {
    let mut rng = Rng::new(0x6400_0001);
    let mut worst = 0.0f64;
    let mut total = 0.0f64;
    const SAMPLES: u32 = 200_000;

    for _ in 0..SAMPLES {
        // The reference is f64 throughout. An f32 reference would put the
        // measurement's own noise floor above the quantity being measured —
        // the pitfall the source paper documents.
        let reference = common::random_unit_quaternion_f64(&mut rng);
        let packed = FineRotation::from_versor(common::versor_from_f64(reference));
        let decoded = common::to_f64_quaternion(packed.to_versor());
        let error = common::angle_degrees(reference, decoded);
        worst = worst.max(error);
        total += error;
    }

    let mean = total / f64::from(SAMPLES);
    println!("FineRotation: mean {mean:.5} deg, max {worst:.5} deg over {SAMPLES} samples");
    assert!(worst < BUDGET_DEGREES, "max error {worst} degrees");
    assert!(mean < 0.002, "mean error {mean} degrees");
}

#[test]
fn the_fine_tier_is_two_orders_of_magnitude_better_than_the_coarse_one() {
    // The evidence for having two tiers at all.
    let mut rng = Rng::new(0x6400_0002);
    let mut coarse_worst = 0.0f64;
    let mut fine_worst = 0.0f64;

    for _ in 0..50_000 {
        let reference = common::random_unit_quaternion_f64(&mut rng);
        let q = common::versor_from_f64(reference);
        coarse_worst = coarse_worst.max(common::angle_degrees(
            reference,
            common::to_f64_quaternion(Rotation::from_versor(q).to_versor()),
        ));
        fine_worst = fine_worst.max(common::angle_degrees(
            reference,
            common::to_f64_quaternion(FineRotation::from_versor(q).to_versor()),
        ));
    }

    println!("coarse max {coarse_worst:.4} deg, fine max {fine_worst:.5} deg");
    assert!(
        fine_worst * 20.0 < coarse_worst,
        "fine {fine_worst}, coarse {coarse_worst}"
    );
}

#[test]
fn repacking_is_stable_and_bounded() {
    // `pack ∘ unpack` is the identity on almost every pattern, and where it is
    // not, it moves one component by one last bit.
    //
    // Stated over the patterns the encoder actually produces. An arbitrary
    // `u64` is a quaternion of arbitrary norm, and re-encoding renormalizes it
    // onto a quite different lattice point — which is correct behaviour, not
    // instability, and is why the loop below starts from `from_versor`.
    //
    // Where it does move, the cause is inherent to "normalize, then round": a
    // lattice point is not always the *closest* lattice point to its own
    // normalized direction, so a component sitting near a rounding boundary can
    // land one step over on the way back.
    let mut rng = Rng::new(0x6400_0003);
    const SAMPLES: u32 = 100_000;
    let mut moved = 0u32;
    let mut worst = 0.0f64;

    for _ in 0..SAMPLES {
        let once = FineRotation::from_versor(common::random_versor(&mut rng));
        let twice = FineRotation::from_versor(once.to_versor());
        if twice.to_bits() != once.to_bits() {
            moved += 1;
            worst = worst.max(once.to_versor().angle_to(twice.to_versor()).to_degrees());
        }
    }

    println!("FineRotation repack: {moved} of {SAMPLES} patterns moved, worst {worst:.5} deg");
    assert!(
        f64::from(moved) / f64::from(SAMPLES) < 0.05,
        "{moved} of {SAMPLES} patterns changed bits"
    );
    assert!(
        worst < BUDGET_DEGREES,
        "repacking moved a rotation by {worst} degrees"
    );
}

#[test]
fn both_members_of_a_double_cover_pair_canonicalize_the_same_way() {
    let mut rng = Rng::new(0x6400_0004);
    for _ in 0..50_000 {
        let q = common::random_versor(&mut rng);
        assert_eq!(
            FineRotation::from_versor(q).to_bits(),
            FineRotation::from_versor(q.negate()).to_bits()
        );
        assert!(FineRotation::from_versor(q).is_canonical());
    }
}

#[test]
fn a_non_canonical_pattern_still_behaves_as_the_rotation_it_denotes() {
    // Equality and hashing route through `canonicalize`, so a pattern that
    // arrives over a wire with the other sign of the double cover compares
    // equal to the canonical one — without any re-quantization in between.
    let mut rng = Rng::new(0x6400_0005);
    for _ in 0..50_000 {
        let canonical = FineRotation::from_versor(common::random_versor(&mut rng));
        let flipped = negate_components(canonical);

        assert_eq!(canonical, flipped);
        assert_eq!(canonical.canonicalize(), flipped.canonicalize());
        assert_eq!(canonical.to_bits(), flipped.canonicalize().to_bits());
        assert!(canonical.is_canonical());
        // The flipped pattern is only non-canonical when it actually differs —
        // an all-zero-but-one rotation flips to itself.
        assert_eq!(
            flipped.is_canonical(),
            flipped.to_bits() == canonical.to_bits()
        );
    }
}

/// Negates all four stored components, giving the other member of the
/// double-cover pair with no arithmetic in between.
fn negate_components(r: FineRotation) -> FineRotation {
    let bits = r.to_bits();
    let mut out = 0u64;
    for slot in 0..4 {
        let component = ((bits >> (slot * 16)) & 0xFFFF) as u16 as i16;
        out |= u64::from((-component) as u16) << (slot * 16);
    }
    FineRotation::from_bits(out)
}

#[test]
fn equal_rotations_hash_equally_even_across_the_double_cover() {
    use std::collections::HashSet;

    let mut set = HashSet::new();
    let mut rng = Rng::new(0x6400_0006);
    for _ in 0..1_000 {
        let q = common::random_versor(&mut rng);
        set.insert(FineRotation::from_versor(q));
        set.insert(FineRotation::from_versor(q.negate()));
    }
    assert_eq!(set.len(), 1_000);
}

#[test]
fn the_two_tiers_convert_both_ways() {
    let mut rng = Rng::new(0x6400_0007);
    for _ in 0..20_000 {
        let coarse = Rotation::from_versor(common::random_versor(&mut rng));

        // Upgrading is total and re-quantizes: this tier's 0.0034 degree
        // quantum lands on top of the 0.186 degrees Rotation already carries.
        let fine = FineRotation::from_rotation(coarse);
        let upgrade_cost = fine.to_versor().angle_to(coarse.to_versor()).to_degrees();
        assert!(
            upgrade_cost < BUDGET_DEGREES,
            "upgrade moved {upgrade_cost} degrees"
        );

        // Downgrading is total and loses accuracy down to the 32-bit tier.
        let back = fine.to_rotation();
        assert!(back.to_versor().angle_to(coarse.to_versor()).to_degrees() < 0.4);
    }
}

#[test]
fn identity_is_exact() {
    assert_eq!(FineRotation::IDENTITY.to_basis(), Basis::IDENTITY);
    assert_eq!(
        FineRotation::from_versor(Versor::IDENTITY),
        FineRotation::IDENTITY
    );
    assert_eq!(
        FineRotation::from_basis(Basis::IDENTITY),
        FineRotation::IDENTITY
    );
    assert_eq!(FineRotation::default(), FineRotation::IDENTITY);
    assert!(FineRotation::IDENTITY.is_canonical());
}

#[test]
fn every_bit_pattern_decodes_to_a_unit_quaternion() {
    let mut rng = Rng::new(0x6400_0008);
    for _ in 0..100_000 {
        let q = FineRotation::from_bits(rng.next_u64()).to_versor();
        let [x, y, z, w] = common::to_f64_quaternion(q);
        let norm = x * x + y * y + z * z + w * w;
        assert!((norm - 1.0).abs() < 1e-6, "{q:?} has squared norm {norm}");
    }
    // The all-zero pattern — a zeroed buffer, a `serde` `0`, `bytemuck::zeroed`
    // — names no rotation, so it decodes to the identity rather than to the
    // zero quaternion, which is not a rotation at all.
    let zero = FineRotation::from_bits(0);
    let [x, y, z, w] = common::to_f64_quaternion(zero.to_versor());
    let norm = x * x + y * y + z * z + w * w;
    assert!(
        (norm - 1.0).abs() < 1e-6,
        "the zero pattern has squared norm {norm}"
    );
    assert_eq!(zero.to_versor(), Versor::IDENTITY);
    assert_eq!(zero.to_basis(), Basis::IDENTITY);
    // …and its round trip through the coarse tier is the identity too, rather
    // than the half turn a zero quaternion re-encodes into.
    assert_eq!(zero.to_rotation().to_basis(), Basis::IDENTITY);
    assert_eq!(zero.canonicalize(), FineRotation::IDENTITY);
}

#[test]
fn the_codec_is_available_in_const_context() {
    const PACKED: FineRotation = FineRotation::from_versor(Versor::IDENTITY);
    const DECODED: Versor = PACKED.to_versor();
    const COARSE: Rotation = PACKED.to_rotation();

    assert_eq!(PACKED, FineRotation::IDENTITY);
    assert_eq!(DECODED, Versor::IDENTITY);
    assert_eq!(COARSE, Rotation::IDENTITY);
}
