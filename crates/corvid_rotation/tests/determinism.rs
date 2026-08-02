//! Const-evaluated results against runtime ones, plus golden bit tables.
//!
//! rustc's const interpreter and the CPU are independent implementations of the
//! same integer arithmetic, so their agreement is evidence rather than
//! tautology.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
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

use std::hint::black_box;

use common::Rng;
use corvid_fixed::{Angle32, Factor32, I2F30, Pitch32, Signed32};
use corvid_rotation::{Basis, FineRotation, Rotation, Versor};
use corvid_vector::{Direction, FinePoint};

/// Yaw, pitch and roll in degrees, then the `Rotation` and `FineRotation` bits
/// they pack into. Regenerate with `cargo run --example dump_rotation_golden`.
const GOLDEN_POSES: &[(f64, f64, f64, u32, u64)] = &[
    (0.0, 0.0, 0.0, 3_758_621_184, 9_223_090_561_878_065_152),
    (90.0, 0.0, 0.0, 3_220_701_696, 6_521_874_724_778_147_840),
    (37.0, -12.0, 3.0, 3_935_825_350, 8_703_531_791_151_788_610),
    (180.0, 45.0, 0.0, 2_685_096_448, 130_022_366_707_712),
    (
        -120.0,
        89.0,
        179.0,
        1_064_071_969,
        5_627_014_819_077_443_443,
    ),
];

const fn pose(yaw: f64, pitch: f64, roll: f64) -> Basis {
    Basis::from_yaw_pitch_roll(
        Angle32::from_degrees(yaw),
        Pitch32::from_degrees(pitch),
        Angle32::from_degrees(roll),
    )
}

#[test]
fn the_codecs_match_their_golden_tables() {
    for &(yaw, pitch, roll, coarse, fine) in GOLDEN_POSES {
        let m = pose(yaw, pitch, roll);
        assert_eq!(
            Rotation::from_basis(m).to_bits(),
            coarse,
            "Rotation of ({yaw}, {pitch}, {roll})"
        );
        assert_eq!(
            FineRotation::from_basis(m).to_bits(),
            fine,
            "FineRotation of ({yaw}, {pitch}, {roll})"
        );
    }
}

#[test]
fn const_evaluation_agrees_with_runtime() {
    const AXIS: Direction = Direction::new(Signed32::ZERO, Signed32::ZERO, Signed32::MAX);
    const A: Versor = Versor::from_axis_angle(AXIS, Angle32::QUARTER_TURN);
    const B: Basis =
        Basis::from_yaw_pitch_roll(Angle32::QUARTER_TURN, Pitch32::ZERO, Angle32::ZERO);

    const AS_MATRIX: Basis = A.to_basis();
    const COMPOSED: Basis = B.compose(B);
    const INVERTED: Basis = B.inverse();
    const PACKED: Rotation = Rotation::from_basis(B);
    const FINE: FineRotation = FineRotation::from_basis(B);
    const DECODED: Versor = PACKED.to_versor();
    const ROTATED: FinePoint = B.rotate_fine(FinePoint::ZERO);
    const BLEND: Versor = A.nlerp(Versor::IDENTITY, Factor32::from_f64(0.25));
    const SPHERICAL: Versor = A.slerp(Versor::IDENTITY, Factor32::from_f64(0.25));
    const ANGLE: Angle32 = A.angle_to(Versor::IDENTITY);

    let (a, b) = (black_box(A), black_box(B));
    assert_eq!(AS_MATRIX, a.to_basis());
    assert_eq!(COMPOSED, b.compose(b));
    assert_eq!(INVERTED, b.inverse());
    assert_eq!(PACKED, Rotation::from_basis(b));
    assert_eq!(FINE.to_bits(), FineRotation::from_basis(b).to_bits());
    assert_eq!(DECODED, PACKED.to_versor());
    assert_eq!(ROTATED, b.rotate_fine(FinePoint::ZERO));
    assert_eq!(BLEND, a.nlerp(Versor::IDENTITY, Factor32::from_f64(0.25)));
    assert_eq!(
        SPHERICAL,
        a.slerp(Versor::IDENTITY, Factor32::from_f64(0.25))
    );
    assert_eq!(ANGLE, a.angle_to(Versor::IDENTITY));
}

#[test]
fn the_same_inputs_give_the_same_bits_every_run() {
    let checksum = |seed: u64| {
        let mut rng = Rng::new(seed);
        let mut acc = 0u64;
        for _ in 0..20_000 {
            let q = common::random_versor(&mut rng);
            acc = acc
                .wrapping_mul(31)
                .wrapping_add(u64::from(Rotation::from_versor(q).to_bits()));
            acc = acc
                .wrapping_mul(31)
                .wrapping_add(FineRotation::from_versor(q).to_bits());
            let m = q.to_basis();
            for row in m.to_rows() {
                for entry in row {
                    acc = acc
                        .wrapping_mul(31)
                        .wrapping_add(entry.to_bits() as u32 as u64);
                }
            }
        }
        acc
    };
    // Pinned rather than compared to a rerun of itself: a rerun proves only
    // that the function is a function. A change that moves every result
    // alike — a rounding rule, a normalize retune — fails here.
    assert_eq!(
        checksum(0xDE7_E4A1),
        3_157_927_899_308_932_402,
        "the sequence changed"
    );
}

#[test]
fn decoding_is_bit_stable_across_repeated_calls() {
    // The property the VR stability tests lean on: a fixed packed pose decodes
    // to the same bits every frame, so nothing shimmers.
    let packed = FineRotation::from_basis(pose(37.0, -12.0, 3.0));
    let first = packed.to_basis();
    let first_versor = packed.to_versor();
    for _ in 0..10_000 {
        assert_eq!(packed.to_basis(), first);
        assert_eq!(packed.to_versor(), first_versor);
    }
    let _ = I2F30::ZERO;
}
