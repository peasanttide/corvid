//! Const-evaluated results against runtime ones, plus golden bit tables.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::many_single_char_names,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    reason = "the golden tables are raw bit patterns, converted as such"
)]

mod common;

use std::hint::black_box;

use common::Rng;
use corvid_transform::{
    Factor32, FinePoint, FineRotation, FineTransform, GlobalFinePoint, GlobalPoint, I24F8, I48F16,
    Rotation, Transform,
};

/// Camera position in metres, then the eye-space bits of a point one metre
/// ahead of it on each axis.
const GOLDEN_EYE: &[(f64, [i32; 3])] = &[
    (0.0, [65_536, 65_536, 65_536]),
    (6_371_000.0, [65_536, 65_536, 65_536]),
    (1.0e13, [65_536, 65_536, 65_536]),
];

#[test]
fn the_hot_path_matches_its_golden_table() {
    for &(origin, expected) in GOLDEN_EYE {
        let camera = FineTransform::new(
            GlobalFinePoint::splat(I48F16::from_f64(origin)),
            FineRotation::IDENTITY,
        );
        let target = camera.position() + GlobalFinePoint::splat(I48F16::from_f64(1.0));
        let local = camera
            .to_fine_global(target)
            .expect("one metre is near field");
        assert_eq!(
            [
                local.x().to_bits(),
                local.y().to_bits(),
                local.z().to_bits()
            ],
            expected,
            "world to eye at {origin} m from the origin"
        );
    }
}

#[test]
fn const_evaluation_agrees_with_runtime() {
    const CAMERA: FineTransform = FineTransform::new(
        GlobalFinePoint::splat(I48F16::from_f64(6_371_000.0)),
        FineRotation::IDENTITY,
    );
    const TARGET: GlobalFinePoint = GlobalFinePoint::splat(I48F16::from_f64(6_371_001.0));

    const EYE: Option<FinePoint> = CAMERA.to_fine_global(TARGET);
    const LOCAL: Option<GlobalPoint> = CAMERA.to_local_global(TARGET);
    const WORLD: GlobalFinePoint = CAMERA.to_world(FinePoint::ZERO);
    const INVERTED: FineTransform = CAMERA.inverse();
    const COMPOSED: FineTransform = CAMERA.compose(CAMERA.inverse());
    const COARSE: Option<Transform> = CAMERA.to_coarse_transform();
    const BLEND: FineTransform = CAMERA.lerp(CAMERA, Factor32::from_f64(0.5));

    let camera = black_box(CAMERA);
    let target = black_box(TARGET);
    assert_eq!(EYE, camera.to_fine_global(target));
    assert_eq!(LOCAL, camera.to_local_global(target));
    assert_eq!(WORLD, camera.to_world(FinePoint::ZERO));
    assert_eq!(INVERTED, camera.inverse());
    assert_eq!(COMPOSED, camera.compose(camera.inverse()));
    assert_eq!(COARSE, camera.to_coarse_transform());
    assert_eq!(BLEND, camera.lerp(camera, Factor32::from_f64(0.5)));
}

#[test]
fn const_evaluation_agrees_with_runtime_at_the_coarse_tier() {
    const T: Transform = Transform::new(
        GlobalPoint::splat(I24F8::from_f64(100.0)),
        Rotation::IDENTITY,
    );
    const UPGRADED: FineTransform = T.to_fine_transform();
    const POINT: GlobalPoint = T.transform_point(GlobalPoint::ZERO);
    const BACK: Option<GlobalPoint> = T.to_local(GlobalPoint::ZERO);

    let t = black_box(T);
    assert_eq!(UPGRADED, t.to_fine_transform());
    assert_eq!(POINT, t.transform_point(GlobalPoint::ZERO));
    assert_eq!(BACK, t.to_local(GlobalPoint::ZERO));
}

#[test]
fn the_same_inputs_give_the_same_bits_every_run() {
    let checksum = |seed: u64| {
        let mut rng = Rng::new(seed);
        let mut acc = 0u64;
        for _ in 0..10_000 {
            let camera = common::random_fine_transform(&mut rng, 1.0e13);
            let target = common::near(&mut rng, camera.position(), 10_000.0);
            if let Some(local) = camera.to_fine_global(target) {
                for component in local.to_array() {
                    acc = acc
                        .wrapping_mul(31)
                        .wrapping_add(component.to_bits() as u32 as u64);
                }
            }
            acc = acc
                .wrapping_mul(31)
                .wrapping_add(camera.rotation().to_bits());
        }
        acc
    };
    // Pinned rather than compared to a rerun of itself: a rerun proves only
    // that the function is a function. A change that moves every result
    // alike — a rounding rule, a codec retune — fails here.
    assert_eq!(
        checksum(0xDE7_E4A1),
        13_347_538_525_801_684_715,
        "the sequence changed"
    );
}
