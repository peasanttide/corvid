//! Yaw, pitch and roll: which axis each turns about, and what survives a round
//! trip through the three of them.
//!
//! The composition is ZXY intrinsic, which is a decision the crate documents
//! and this is where it is held to it -- along with what happens at the poles,
//! where roll and yaw stop being distinguishable and one is folded into the
//! other.

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

use common::Rng;
use corvid_fixed::{Angle32, I2F30, Pitch32, Signed32};
use corvid_rotation::Basis;
use corvid_vector::Direction;

const X: Direction = Direction::new(Signed32::MAX, Signed32::ZERO, Signed32::ZERO);
const Y: Direction = Direction::new(Signed32::ZERO, Signed32::MAX, Signed32::ZERO);
const Z: Direction = Direction::new(Signed32::ZERO, Signed32::ZERO, Signed32::MAX);

/// Component tolerance for a direction that has been through a rotation.
const AXIS_TOLERANCE: f64 = 1e-4;
#[test]
fn yaw_turns_about_z_pitch_about_x_roll_about_y() {
    // A quarter turn of yaw takes forward (+Y) onto left (-X).
    let yaw = Basis::from_yaw_pitch_roll(Angle32::from_degrees(90.0), Pitch32::ZERO, Angle32::ZERO);
    assert!(
        common::direction_within(yaw.forward(), common::neg(X), AXIS_TOLERANCE),
        "yaw forward is {:?}",
        yaw.forward()
    );
    assert!(common::direction_within(yaw.up(), Z, AXIS_TOLERANCE));
    assert!(common::direction_within(yaw.right(), Y, AXIS_TOLERANCE));

    // A quarter turn of pitch takes forward (+Y) onto up (+Z), leaving right
    // alone, because pitch is about +X.
    let pitch =
        Basis::from_yaw_pitch_roll(Angle32::ZERO, Pitch32::from_degrees(90.0), Angle32::ZERO);
    assert!(
        common::direction_within(pitch.forward(), Z, AXIS_TOLERANCE),
        "pitch forward is {:?}",
        pitch.forward()
    );
    assert!(common::direction_within(pitch.right(), X, AXIS_TOLERANCE));

    // A quarter turn of roll takes right (+X) onto down (-Z), leaving forward
    // alone, because roll is about +Y.
    let roll =
        Basis::from_yaw_pitch_roll(Angle32::ZERO, Pitch32::ZERO, Angle32::from_degrees(90.0));
    assert!(
        common::direction_within(roll.forward(), Y, AXIS_TOLERANCE),
        "roll forward is {:?}",
        roll.forward()
    );
    assert!(
        common::direction_within(roll.right(), common::neg(Z), AXIS_TOLERANCE),
        "roll right is {:?}",
        roll.right()
    );
}

#[test]
fn all_three_angles_zero_is_the_identity() {
    assert_eq!(
        Basis::from_yaw_pitch_roll(Angle32::ZERO, Pitch32::ZERO, Angle32::ZERO),
        Basis::IDENTITY
    );
}

#[test]
fn euler_composition_is_zxy_intrinsic() {
    // R = Rz(yaw) . Rx(pitch) . Ry(roll), spelled out as three composes.
    let mut rng = Rng::new(0x2A40_0001);
    for _ in 0..5_000 {
        let yaw = Angle32::from_bits(rng.next_u32());
        let pitch = Pitch32::from_degrees(rng.next_unit() * 89.0);
        let roll = Angle32::from_bits(rng.next_u32());

        let combined = Basis::from_yaw_pitch_roll(yaw, pitch, roll);
        let separate = Basis::from_yaw_pitch_roll(yaw, Pitch32::ZERO, Angle32::ZERO)
            .compose(Basis::from_yaw_pitch_roll(
                Angle32::ZERO,
                pitch,
                Angle32::ZERO,
            ))
            .compose(Basis::from_yaw_pitch_roll(
                Angle32::ZERO,
                Pitch32::ZERO,
                roll,
            ));

        assert!(
            combined.abs_diff_eq(separate, I2F30::from_bits(1 << 14)),
            "ZXY composition disagrees:\n  {combined:?}\n  {separate:?}"
        );
    }
}

#[test]
fn yaw_pitch_roll_round_trips() {
    let mut rng = Rng::new(0x0B50_0001);
    for _ in 0..20_000 {
        let yaw = Angle32::from_bits(rng.next_u32());
        // Stay off the gimbal-lock poles, where yaw and roll are degenerate.
        let pitch = Pitch32::from_degrees(rng.next_unit() * 85.0);
        let roll = Angle32::from_bits(rng.next_u32());

        let m = Basis::from_yaw_pitch_roll(yaw, pitch, roll);
        let (y2, p2, r2) = m.to_yaw_pitch_roll();
        let back = Basis::from_yaw_pitch_roll(y2, p2, r2);
        assert!(
            m.abs_diff_eq(back, I2F30::from_bits(1 << 16)),
            "round trip lost the rotation:\n  {m:?}\n  {back:?}"
        );
    }
}

/// The band just short of the pole, which the round-trip test above skips.
///
/// The degenerate branch throws roll away, so it must not fire while roll is
/// still determined. It used to fire from 89.84 deg, where `cos(pitch)` is
/// `2.8e-3` and the discarded roll cost 0.30 deg of round-trip error -- 60x the
/// 0.005 deg the codec itself carries -- right where a head-tracked pose looking
/// nearly straight up lives.
#[test]
fn near_the_poles_roll_is_still_recovered() {
    for &pitch_degrees in &[89.0, 89.5, 89.83, 89.85, 89.9, 89.95] {
        for yi in 0..17 {
            for ri in 0..17 {
                let yaw = Angle32::from_degrees(-180.0 + 360.0 * f64::from(yi) / 17.0);
                let roll = Angle32::from_degrees(-180.0 + 360.0 * f64::from(ri) / 17.0);
                let pitch = Pitch32::from_degrees(pitch_degrees);

                let m = Basis::from_yaw_pitch_roll(yaw, pitch, roll);
                let (y2, p2, r2) = m.to_yaw_pitch_roll();
                let back = Basis::from_yaw_pitch_roll(y2, p2, r2);
                let error = m.angle_to(back).to_degrees();
                assert!(
                    error < 0.01,
                    "pitch {pitch_degrees}, yaw {yaw:?}, roll {roll:?}: lost {error} deg"
                );
            }
        }
    }
}

#[test]
fn at_the_poles_roll_is_folded_into_yaw() {
    // Pitch at a quarter turn leaves only yaw + roll determined; the whole turn
    // is attributed to yaw and roll comes back zero. The rotation still round-
    // trips, which is what actually matters.
    let straight_up = Basis::from_yaw_pitch_roll(
        Angle32::from_degrees(30.0),
        Pitch32::MAX,
        Angle32::from_degrees(40.0),
    );
    let (_, pitch, roll) = straight_up.to_yaw_pitch_roll();
    assert_eq!(roll, Angle32::ZERO);
    assert!((pitch.to_degrees() - 90.0).abs() < 0.01);

    let (y, p, r) = straight_up.to_yaw_pitch_roll();
    assert!(straight_up.abs_diff_eq(
        Basis::from_yaw_pitch_roll(y, p, r),
        I2F30::from_bits(1 << 16)
    ));

    // The other pole, where the free parameter is yaw - roll rather than
    // yaw + roll and the recovered sine comes back negated.
    let straight_down = Basis::from_yaw_pitch_roll(
        Angle32::from_degrees(30.0),
        Pitch32::MIN,
        Angle32::from_degrees(40.0),
    );
    let (y, p, r) = straight_down.to_yaw_pitch_roll();
    assert_eq!(r, Angle32::ZERO);
    assert!((p.to_degrees() + 90.0).abs() < 0.01);
    assert!(straight_down.abs_diff_eq(
        Basis::from_yaw_pitch_roll(y, p, r),
        I2F30::from_bits(1 << 16)
    ));
}
