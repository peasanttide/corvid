//! The anchor arithmetic, at both scales, and the pose vocabulary under it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    reason = "these tests build fixtures out of raw bit patterns and read matrices back as floats; every cast here is the thing under test rather than an oversight"
)]

use core::time::Duration;

use corvid_fixed::{I16F16, I48F16, Pitch32};
use corvid_hash::digest;
use corvid_rotation::{FineRotation, Versor};
use corvid_vector::{FinePoint, GlobalFinePoint, globalfinepoint};
use corvid_xr::{Anchor, Confidence, Pose, Scale, Tracked};

/// The planet the spec names: 2 856 m of radius, so 5 712 m across.
const ACROSS: I16F16 = I16F16::from_f64(5_712.0);

/// A hundred stage poses, the same hundred on every machine.
///
/// A linear congruential generator rather than a crate: the point is that the
/// sequence is fixed and readable, not that it is random.
fn poses() -> Vec<Pose> {
    let mut seed = 0x2545_F491u32;
    let mut next = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed
    };
    (0..100)
        .map(|_| {
            let axis = |bits: u32| {
                // Within twenty metres of the stage's origin, either way.
                I16F16::from_bits((bits % (40 << 16)) as i32 - (20 << 16))
            };
            let position = FinePoint::new(axis(next()), axis(next()), axis(next()));
            let facing = FineRotation::from_versor(Versor::from_yaw_pitch_roll(
                corvid_fixed::Angle32::from_bits(next()),
                Pitch32::from_bits((next() >> 1) as i32),
                corvid_fixed::Angle32::from_bits(next()),
            ));
            Pose::new(position, facing)
        })
        .collect()
}

/// The largest per-axis difference between two positions, in `I48F16` steps.
fn steps_apart(a: GlobalFinePoint, b: GlobalFinePoint) -> i64 {
    a.to_array()
        .iter()
        .zip(b.to_array())
        .map(|(left, right)| (left.to_bits() - right.to_bits()).abs())
        .max()
        .unwrap_or(0)
}

/// An anchor at human scale, somewhere that is not the origin.
fn standing() -> Anchor {
    Anchor::standing(
        globalfinepoint(1_234_567, -890_123, 2_856),
        FineRotation::from_versor(Versor::from_yaw_pitch_roll(
            corvid_fixed::Angle32::from_degrees(37.0),
            Pitch32::ZERO,
            corvid_fixed::Angle32::ZERO,
        )),
    )
}

/// The planet held as a model a metre across, an arm's length ahead.
fn holding() -> Anchor {
    let ahead = Pose::new(
        FinePoint::new(I16F16::ZERO, I16F16::from_f64(0.6), I16F16::from_f64(1.4)),
        FineRotation::IDENTITY,
    );
    Anchor::holding(globalfinepoint(0, 0, 0), ACROSS, I16F16::ONE, ahead)
}

#[test]
fn to_world_then_to_stage_is_the_identity_at_both_scales() {
    for anchor in [standing(), holding()] {
        let mut worst = 0;
        for pose in poses() {
            let there = anchor.to_world(pose);
            let back = anchor.to_stage(there.position());
            worst = worst.max(steps_apart(pose.origin(), back.origin()));
        }
        // One step of `I48F16` is 15.26 um of stage. At table scale that is
        // 87 mm of world, which the division by `metres` is what shrinks back
        // down: the round trip is as exact in the player's hands as it is under
        // their feet.
        assert!(
            worst <= 1,
            "round trip drifted {worst} steps at metres = {:?}",
            anchor.metres
        );
    }
}

#[test]
fn standing_at_the_origin_unturned_is_the_identity() {
    let anchor = Anchor::standing(GlobalFinePoint::ZERO, FineRotation::IDENTITY);
    for pose in poses() {
        let there = anchor.to_world(pose);
        assert_eq!(there.position(), pose.origin());
        assert_eq!(there.rotation(), pose.rotation());
    }
}

#[test]
fn holding_a_planet_as_a_metre_wide_model_scales_by_five_thousand_seven_hundred_and_twelve() {
    let anchor = holding();
    assert_eq!(anchor.metres, ACROSS);

    // One step of the stage -- 15.26 um -- is 5 712 of them in the world, so a
    // stage millimetre is 5.712 m. That is decision three's arithmetic, and it
    // is why pointing at table scale is a raycast: a cell seventeen
    // micrometres across on the model is not something a hand can pick out.
    let step = FinePoint::new(I16F16::DELTA, I16F16::ZERO, I16F16::ZERO);
    let here = anchor.to_world(Pose::IDENTITY);
    let there = anchor.to_world(Pose::new(step, FineRotation::IDENTITY));
    let moved = there.position().x().to_f64() - here.position().x().to_f64();
    assert!(
        (moved / I48F16::DELTA.to_f64() - 5_712.0).abs() < 1.0,
        "a stage step moved {moved} m of world"
    );
    let millimetre = moved * 65.536;
    assert!(
        (millimetre - 5.712).abs() < 0.001,
        "a stage millimetre moved {millimetre} m of world"
    );

    // And the centre of the model is where it was asked to be.
    assert_eq!(
        anchor.to_stage(GlobalFinePoint::ZERO).position(),
        FinePoint::new(I16F16::ZERO, I16F16::from_f64(0.6), I16F16::from_f64(1.4))
    );
}

#[test]
fn lerp_is_exact_at_both_ends() {
    let (from, to) = (standing(), holding());
    assert_eq!(from.lerp(to, corvid_fixed::Factor16::MIN), from);
    assert_eq!(from.lerp(to, corvid_fixed::Factor16::MAX), to);
    // And the dive between them stays between them.
    let half = from.lerp(to, corvid_fixed::Factor16::from_f64(0.5));
    assert!(half.metres > from.metres && half.metres < to.metres);
}

#[test]
fn a_dive_out_and_back_returns_to_the_anchor_it_left() {
    let (surface, table) = (standing(), holding());
    let out = surface.lerp(table, corvid_fixed::Factor16::MAX);
    let back = out.lerp(surface, corvid_fixed::Factor16::MAX);
    assert_eq!(digest(&back), digest(&surface));
}

#[test]
fn fifteen_micrometres_at_ten_thousand_kilometres_is_a_different_pose() {
    let anchor = Anchor::standing(globalfinepoint(10_000_000, 0, 0), FineRotation::IDENTITY);
    let metre = FinePoint::new(I16F16::ZERO, I16F16::ONE, I16F16::ZERO);
    let nudged = metre.add(FinePoint::new(I16F16::DELTA, I16F16::ZERO, I16F16::ZERO));

    let here = anchor.to_world(Pose::new(metre, FineRotation::IDENTITY));
    let there = anchor.to_world(Pose::new(nudged, FineRotation::IDENTITY));

    assert_ne!(here.position(), there.position());
    assert_eq!(steps_apart(here.position(), there.position()), 1);
    // One step of `I48F16` is 15.26 um, ten thousand kilometres out.
    assert!((I48F16::DELTA.to_f64() - 0.000_015_258_789_062_5).abs() < f64::EPSILON);
}

#[test]
fn a_zero_scale_converts_nothing_rather_than_dividing_by_zero() {
    let anchor = Anchor::default().with_metres(I16F16::ZERO);
    assert_eq!(
        anchor
            .to_world(Pose::new(
                FinePoint::new(I16F16::ONE, I16F16::ONE, I16F16::ONE),
                FineRotation::IDENTITY,
            ))
            .position(),
        GlobalFinePoint::ZERO
    );
    // Saturating rather than panicking, which is what the arithmetic under it
    // does.
    assert_eq!(
        anchor.to_stage(globalfinepoint(1, 0, 0)).position().x(),
        I16F16::MAX
    );
}

#[test]
fn believed_is_none_only_on_lost() {
    let at = Duration::from_millis(7);
    assert_eq!(
        Tracked::new(1u8, Confidence::Lost, at).believed(),
        None,
        "a lost reading is not believed"
    );
    for confidence in [Confidence::Inferred, Confidence::Tracked] {
        assert_eq!(Tracked::new(1u8, confidence, at).believed(), Some(1));
    }
    assert!(Tracked::tracked(1u8, at).is_tracked());
    assert!(!Tracked::inferred(1u8, at).is_tracked());
    assert!(!Tracked::lost(1u8, at).is_tracked());
}

#[test]
fn a_reading_is_worth_no_more_than_the_weaker_of_two() {
    let at = Duration::from_millis(7);
    let strong = Tracked::tracked(1u8, at);
    assert_eq!(
        strong.capped(Confidence::Inferred).confidence,
        Confidence::Inferred
    );
    assert_eq!(
        strong.capped(Confidence::Tracked).confidence,
        Confidence::Tracked
    );
    assert_eq!(
        Tracked::lost(1u8, at)
            .capped(Confidence::Tracked)
            .confidence,
        Confidence::Lost
    );
}

#[test]
fn map_carries_the_confidence_and_the_time_through_a_conversion() {
    let reading = Tracked::inferred(2u8, Duration::from_millis(9));
    let mapped = reading.map(u32::from);
    assert_eq!(mapped.value, 2u32);
    assert_eq!(mapped.confidence, reading.confidence);
    assert_eq!(mapped.at, reading.at);
}

#[test]
fn everything_here_hashes_so_a_pose_can_be_frozen() {
    let anchor = holding();
    assert_eq!(digest(&anchor), digest(&anchor.clone()));
    assert_ne!(digest(&anchor), digest(&standing()));
    assert_ne!(digest(&Scale::Table), digest(&Scale::Surface));
    assert!(!Scale::Table.is_human());
    assert!(Scale::Surface.is_human());
}
