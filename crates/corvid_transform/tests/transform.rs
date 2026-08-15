//! Round trips, composition order, and the earth-scale claim.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_sign_loss,
    clippy::suboptimal_flops,
    clippy::float_cmp,
    reason = "these tests feed edge-case bit patterns through narrowing casts on purpose, and compare against exactly representable references"
)]

mod common;

use core::mem::size_of;

use common::Rng;
use corvid_transform::{
    FineRotation, FineTransform, GlobalFinePoint, GlobalPoint, I16F16, I24F8, I48F16, Rotation,
    StageTransform, Transform,
};

#[test]
fn the_sizes_are_contractual() {
    assert_eq!(size_of::<Transform>(), 16);
    assert_eq!(size_of::<FineTransform>(), 32);
    // A stage pose keeps the fine rotation and drops the world range: twelve
    // bytes of position, four of hole, eight of rotation.
    assert_eq!(size_of::<StageTransform>(), 24);
}

#[test]
fn identity_leaves_every_point_alone() {
    let p = GlobalPoint::new(
        I24F8::from_f64(1.0),
        I24F8::from_f64(2.0),
        I24F8::from_f64(3.0),
    );
    assert_eq!(Transform::IDENTITY.transform_point(p), p);
    assert_eq!(Transform::IDENTITY.to_local(p), Some(p));
    assert_eq!(Transform::IDENTITY.inverse(), Transform::IDENTITY);
    assert_eq!(Transform::default(), Transform::IDENTITY);
    assert_eq!(FineTransform::default(), FineTransform::IDENTITY);
}

#[test]
fn inverse_composed_with_the_original_is_the_identity() {
    let mut rng = Rng::new(0x7A00_0001);
    for _ in 0..20_000 {
        // 4000 km per axis keeps the *length* inside GlobalPoint's +-8388 km,
        // which the inverse's own position needs: it stores `-R^-1 t`, whose
        // length is |t|. Beyond that the position saturates, as `inverse`
        // documents.
        let t = common::random_transform(&mut rng, 4_000_000.0);

        // The residual position error is not a constant: a transform stores a
        // *quantized* rotation, so inverting one whose position is `d` from the
        // origin leaves up to `d * quantum` of position error -- 0.186 degrees
        // is 3.2e-3 radians, which at 8000 km is tens of kilometres. That is a
        // property of the coarse tier, not a defect, and `Transform::inverse`
        // documents it.
        let quantum_radians = 0.2_f64.to_radians();
        let budget = t.position().length().to_f64() * quantum_radians + 1.0;

        assert!(
            common::transform_near_identity(t.inverse().compose(t), budget, 0.5),
            "{t:?} did not invert"
        );
        assert!(common::transform_near_identity(
            t.compose(t.inverse()),
            budget,
            0.5
        ));
    }
}

#[test]
fn inverting_a_transform_near_the_origin_is_tight() {
    // The same operation where the rotation quantum has nothing to amplify.
    let mut rng = Rng::new(0x7A00_0007);
    for _ in 0..20_000 {
        let t = common::random_transform(&mut rng, 10.0);
        assert!(
            common::transform_near_identity(t.inverse().compose(t), 0.1, 0.5),
            "{t:?} did not invert"
        );
    }

    // And the fine tier holds far tighter, because its quantum is 55x smaller
    // and its position resolution 256x finer.
    let mut rng = Rng::new(0x7A00_0008);
    for _ in 0..5_000 {
        let t = common::random_fine_transform(&mut rng, 10.0);
        let round_tripped = t.inverse().compose(t);
        assert!(round_tripped.position().length().to_f64() < 0.001);
        assert!(
            round_tripped
                .rotation()
                .to_versor()
                .angle_to(corvid_transform::Versor::IDENTITY)
                .to_degrees()
                < 0.02
        );
    }
}

#[test]
fn compose_applies_the_right_hand_operand_first() {
    // If this ever flips, the test fails rather than the geometry silently
    // mirroring.
    let mut rng = Rng::new(0x7A00_0002);
    for _ in 0..10_000 {
        let a = common::random_transform(&mut rng, 1000.0);
        let b = common::random_transform(&mut rng, 1000.0);
        let p = GlobalPoint::new(
            I24F8::from_f64(rng.next_unit() * 100.0),
            I24F8::from_f64(rng.next_unit() * 100.0),
            I24F8::from_f64(rng.next_unit() * 100.0),
        );
        let composed = a.compose(b).transform_point(p);
        let sequential = a.transform_point(b.transform_point(p));
        assert!(
            common::points_within(composed, sequential, I24F8::from_f64(2.0)),
            "compose applied its operands in the wrong order: {composed:?} vs {sequential:?}"
        );
    }
}

#[test]
fn a_camera_at_earth_radius_still_resolves_the_near_field() {
    // 6.37e6 m from the origin, looking at something a millimetre away.
    let camera = FineTransform::new(
        GlobalFinePoint::splat(I48F16::from_f64(6_371_000.0)),
        FineRotation::IDENTITY,
    );
    let target = GlobalFinePoint::new(
        camera.position().x() + I48F16::from_f64(0.001),
        camera.position().y(),
        camera.position().z(),
    );
    let local = camera
        .to_fine_global(target)
        .expect("1 mm is in near-field range");

    // 15.26 um resolution survives the trip: the answer is the exact bit
    // pattern of the difference, not an approximation of it.
    assert_eq!(
        i64::from(local.x().to_bits()),
        I48F16::from_f64(0.001).to_bits(),
        "the near field lost precision at earth radius"
    );
    assert_eq!(local.y(), I16F16::ZERO);
    assert_eq!(local.z(), I16F16::ZERO);
}

#[test]
fn world_to_eye_is_bit_exact_before_the_rotation() {
    // Steps 1-3 introduce no rounding at all. Prove it with the identity
    // rotation, where step 4 is exact too, and walk the whole fractional space
    // of one component at 1e13 m from the origin.
    let camera = FineTransform::new(
        GlobalFinePoint::splat(I48F16::from_f64(1.0e13)),
        FineRotation::IDENTITY,
    );
    for frac in 0..=u16::MAX {
        let offset = I48F16::from_bits(i64::from(frac));
        let target = GlobalFinePoint::new(
            camera.position().x() + offset,
            camera.position().y(),
            camera.position().z(),
        );
        let local = camera.to_fine_global(target).expect("in range");
        assert_eq!(i64::from(local.x().to_bits()), i64::from(frac));
    }
}

#[test]
fn none_appears_only_when_the_difference_leaves_range() {
    let camera = FineTransform::new(GlobalFinePoint::ZERO, FineRotation::IDENTITY);

    let inside = GlobalFinePoint::splat(I48F16::from_f64(30_000.0));
    assert!(camera.to_fine_global(inside).is_some());

    let outside = GlobalFinePoint::splat(I48F16::from_f64(40_000.0));
    assert_eq!(camera.to_fine_global(outside), None);

    // The same point is fine through to_local, which has the wider range.
    assert!(camera.to_local_global(outside).is_some());

    // And a point past even that fails both.
    let very_far = GlobalFinePoint::splat(I48F16::from_f64(1.0e10));
    assert_eq!(camera.to_fine_global(very_far), None);
    assert_eq!(camera.to_local_global(very_far), None);
}

#[test]
fn to_world_undoes_to_fine() {
    let mut rng = Rng::new(0x7A00_0003);
    for _ in 0..20_000 {
        let camera = common::random_fine_transform(&mut rng, 1.0e13);
        let target = common::near(&mut rng, camera.position(), 10_000.0);
        let local = camera.to_fine_global(target).expect("near field");
        let back = camera.to_world(local);
        // One rotation rounding each way, scaled by the offset's magnitude.
        assert!(
            common::fine_points_within(back, target, I48F16::from_f64(0.02)),
            "round trip moved the point: {target:?} -> {back:?}"
        );
    }
}

#[test]
fn to_local_and_to_fine_agree_where_both_are_defined() {
    let mut rng = Rng::new(0x7A00_0004);
    for _ in 0..20_000 {
        let camera = common::random_fine_transform(&mut rng, 1.0e6);
        let target = common::near(&mut rng, camera.position(), 20_000.0);
        let fine = camera.to_fine_global(target).expect("near field");
        let coarse = camera.to_local_global(target).expect("near field");
        // to_local's own 3.9 mm resolution is the only difference.
        assert!(
            (fine.x().to_f64() - coarse.x().to_f64()).abs() < 0.01,
            "{} vs {}",
            fine.x().to_f64(),
            coarse.x().to_f64()
        );
    }
}

#[test]
fn to_fine_transform_is_total_and_says_what_it_costs() {
    let mut rng = Rng::new(0x7A00_0005);
    for _ in 0..20_000 {
        let t = common::random_transform(&mut rng, 8_000_000.0);
        let fine = t.to_fine_transform();

        // The position widens exactly.
        assert_eq!(fine.position().x().to_f64(), t.position().x().to_f64());
        assert_eq!(fine.position(), t.position().to_global_fine());

        // The rotation is re-quantized: it moves by at most FineRotation's
        // own quantum, which is what "not a free upgrade" means.
        let drift = fine
            .rotation()
            .to_versor()
            .angle_to(t.rotation().to_versor())
            .to_degrees();
        assert!(drift < 1.0 / 128.0, "upgrade drifted {drift} degrees");

        // And it round-trips back to the same position and the same rotation.
        // Not necessarily the same *bits*: a rotation whose two largest
        // quaternion components tie in magnitude has two equally valid charts,
        // and re-encoding is free to pick either -- see `corvid_rotation`'s
        // `repacking_is_stable_and_bounded`.
        let back = fine
            .to_coarse_transform()
            .expect("the position widened exactly");
        assert_eq!(back.position(), t.position());
        assert!(
            back.rotation()
                .to_versor()
                .angle_to(t.rotation().to_versor())
                .to_degrees()
                < 0.2,
            "the round trip changed the rotation"
        );
    }
}

#[test]
fn to_coarse_transform_fails_only_on_position_range() {
    let far = FineTransform::new(
        GlobalFinePoint::splat(I48F16::from_f64(1.0e13)),
        FineRotation::IDENTITY,
    );
    assert_eq!(far.to_coarse_transform(), None);

    // Just inside GlobalPoint's range succeeds, however coarse the rotation
    // becomes.
    let inside = FineTransform::new(
        GlobalFinePoint::splat(I48F16::from_f64(8_000_000.0)),
        common::pose(37.0, -12.0, 3.0),
    );
    let coarse = inside.to_coarse_transform().expect("inside +-8388 km");
    assert!(
        coarse
            .rotation()
            .to_versor()
            .angle_to(inside.rotation().to_versor())
            .to_degrees()
            < 0.2
    );

    // The trait conversions agree.
    assert!(Transform::try_from(far).is_err());
    assert_eq!(
        FineTransform::from(Transform::IDENTITY),
        FineTransform::IDENTITY
    );
}

#[test]
fn the_local_path_never_touches_i128() {
    // Not something a test can observe directly, so assert the property that
    // makes it true: the offset always fits FinePoint before the rotation, and
    // the rotation of a FinePoint is bounded by the i64 invariant.
    let mut rng = Rng::new(0x7A00_0006);
    for _ in 0..20_000 {
        let camera = common::random_fine_transform(&mut rng, 1.0e13);
        let target = common::near(&mut rng, camera.position(), 30_000.0);
        if let Some(near) = target
            .checked_sub(camera.position())
            .and_then(corvid_transform::GlobalFinePoint::to_fine)
        {
            // Every component fits i32, so the row sums fit i64.
            for component in near.to_array() {
                assert!(i64::from(component.to_bits()).abs() <= i64::from(i32::MAX));
            }
        }
    }
}

#[test]
fn the_transform_family_is_available_in_const_context() {
    const T: Transform = Transform::IDENTITY;
    const F: FineTransform = FineTransform::IDENTITY;
    const COMPOSED: Transform = T.compose(T);
    const INVERTED: Transform = T.inverse();
    const UPGRADED: FineTransform = T.to_fine_transform();
    const LOCAL: Option<GlobalPoint> = T.to_local(GlobalPoint::ZERO);
    const NEAR: Option<corvid_transform::FinePoint> = F.to_fine_global(GlobalFinePoint::ZERO);

    assert_eq!(COMPOSED, Transform::IDENTITY);
    assert_eq!(INVERTED, Transform::IDENTITY);
    assert_eq!(UPGRADED, FineTransform::IDENTITY);
    assert_eq!(LOCAL, Some(GlobalPoint::ZERO));
    assert_eq!(NEAR, Some(corvid_transform::FinePoint::ZERO));
    assert_eq!(Rotation::IDENTITY, T.rotation());
}
