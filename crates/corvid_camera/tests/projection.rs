//! What a screen position denotes, and which way a field of view widens.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_camera::{Camera, FirstPerson};
use corvid_fixed::{Angle16, Angle32, I16F16, I24F8, Pitch32, Signed16, Signed32};
use corvid_rotation::{FineRotation, Versor};
use corvid_shape::{Frustum, Ray};
use corvid_transform::FineTransform;
use corvid_vector::{Direction, GlobalFinePoint, GlobalPoint, globalfinepoint, globalpoint};

/// At the origin, facing forward, with the default frustum.
const fn eye() -> Camera {
    at(FineTransform::new(
        GlobalFinePoint::ZERO,
        FineRotation::IDENTITY,
    ))
}

/// A camera at a given pose, with the default frustum.
const fn at(pose: FineTransform) -> Camera {
    let mut camera = FirstPerson::new(pose.position().to_global().unwrap());
    camera.facing = pose.rotation();
    camera.camera()
}

/// The middle of the screen.
const CENTRE: (Signed16, Signed16) = (Signed16::ZERO, Signed16::ZERO);

/// A square window.
const SQUARE: I16F16 = I16F16::ONE;

/// A ray's slope on one axis against its forward component.
fn slope(ray: Ray, axis: usize) -> f64 {
    let components = ray.direction.to_array();
    components[axis].to_f64() / components[1].to_f64()
}

/// The centre of the screen is straight ahead, which is +Y in this workspace's
/// convention.
#[test]
fn the_centre_looks_forward() {
    let ray = eye().ray(CENTRE, SQUARE);
    assert_eq!(ray.direction, Direction::Y);
}

/// The ray starts at the eye, not at the near plane. A cursor cast that began
/// at the near plane reports a distance short by the near distance on every
/// pick, forever.
#[test]
fn the_ray_starts_at_the_eye() {
    let pose = FineTransform::new(globalfinepoint(1, 2, 3), FineRotation::IDENTITY);
    let ray = at(pose).ray(CENTRE, SQUARE);
    assert_eq!(ray.origin, globalpoint(1, 2, 3));
}

/// Half the vertical field of view is what the top of the screen is at. At 60 degrees
/// that is 30 degrees, whose tangent is 0.5774 -- so the ray's up over its forward is
/// that.
#[test]
fn the_top_edge_is_half_the_fov_up() {
    let top = eye().ray((Signed16::ZERO, Signed16::MAX), SQUARE);
    let rise = slope(top, 2);
    assert!((rise - 0.577_350).abs() < 1e-3, "{rise}");
}

/// And the bottom is the same the other way, exactly.
#[test]
fn the_screen_is_symmetric() {
    let top = eye().ray((Signed16::ZERO, Signed16::MAX), SQUARE);
    let bottom = eye().ray((Signed16::ZERO, Signed16::MIN), SQUARE);
    assert!((slope(top, 2) + slope(bottom, 2)).abs() < 1e-3);
}

/// `slope` is the tangent of half the field of view, which is the one
/// trigonometric quantity a projection has.
#[test]
fn the_slope_is_the_half_angle_tangent() {
    let ninety = Frustum::default().with_fov(Angle16::from_degrees(90.0));
    // tan 45 degrees is one.
    assert!(
        (ninety.slope.to_f64() - 1.0).abs() < 1e-3,
        "{:?}",
        ninety.slope
    );

    let sixty = Frustum::default();
    assert!((sixty.slope.to_f64() - 0.577_350).abs() < 1e-3);
}

/// Half a turn has an infinite tangent, and saturates rather than dividing by
/// zero. A frustum nobody wants is not a panic.
#[test]
fn a_degenerate_fov_saturates() {
    let flat = Frustum::default().with_fov(Angle16::from_degrees(180.0));
    assert_eq!(flat.slope, I16F16::MAX);
}

/// A wider window widens the horizontal field of view and leaves the vertical
/// one alone. This is hor-plus, and it is the assertion that fails if the
/// stored angle is ever swapped for the horizontal one.
#[test]
fn aspect_widens_horizontally() {
    let square = eye().ray((Signed16::MAX, Signed16::ZERO), SQUARE);
    let wide = eye().ray((Signed16::MAX, Signed16::ZERO), I16F16::from_f64(2.0));
    assert!(
        slope(wide, 0) > slope(square, 0) * 1.9,
        "{} vs {}",
        slope(wide, 0),
        slope(square, 0)
    );

    // And the vertical is untouched by it.
    let tall_square = eye().ray((Signed16::ZERO, Signed16::MAX), SQUARE);
    let tall_wide = eye().ray((Signed16::ZERO, Signed16::MAX), I16F16::from_f64(2.0));
    assert!((slope(tall_square, 2) - slope(tall_wide, 2)).abs() < 1e-3);
}

/// The camera's own rotation carries the ray with it.
#[test]
fn turning_the_camera_turns_the_ray() {
    let quarter = FineRotation::from_versor(Versor::from_yaw_pitch_roll(
        Angle32::from_turns(0.25),
        Pitch32::ZERO,
        Angle32::ZERO,
    ));
    let pose = FineTransform::new(GlobalFinePoint::ZERO, quarter);
    let ray = at(pose).ray(CENTRE, SQUARE);

    // A quarter turn about up takes forward to -X in a right-handed frame with
    // +X right, +Y forward and +Z up.
    assert_eq!(ray.direction, -Direction::X);
}

/// A camera looking up sends the middle of the screen up.
#[test]
fn pitching_the_camera_pitches_the_ray() {
    let raised = FineRotation::from_versor(Versor::from_yaw_pitch_roll(
        Angle32::ZERO,
        Pitch32::from_turns(0.1),
        Angle32::ZERO,
    ));
    let pose = FineTransform::new(GlobalFinePoint::ZERO, raised);
    let ray = at(pose).ray(CENTRE, SQUARE);
    assert!(ray.direction.z() > Signed32::ZERO, "{:?}", ray.direction);
}

/// The default frustum is one a game can draw through without setting it.
#[test]
fn the_default_is_a_sensible_frustum() {
    let frustum = Frustum::default();
    assert!(frustum.near > I16F16::ZERO);
    assert!(frustum.far > frustum.near);
    assert_eq!(
        frustum.base,
        I16F16::ZERO,
        "the default is a perspective frustum"
    );

    // The angle survives the round trip through the stored slope.
    let recovered = frustum.fov_y().to_degrees();
    assert!((recovered - 60.0).abs() < 0.5, "{recovered}");
}

/// A camera further from the origin than a [`GlobalPoint`] reaches still casts
/// from where it is, as nearly as the type allows, rather than from the origin.
///
/// The regression this pins was silent: `to_global` answers `None` past
/// 8388 km and the fallback was `GlobalPoint::ZERO`, so a camera on the far
/// side of an Earth-sized planet -- the case this crate's own documentation is
/// written around -- picked along a ray starting at the planet's centre. The
/// direction was right the whole time, which is what made it look like a bug in
/// the geometry rather than in the conversion.
#[test]
fn a_camera_past_the_global_range_casts_from_the_edge_not_the_origin() {
    let far = globalfinepoint(9_000_000, 0, 0);
    assert!(
        far.to_global().is_none(),
        "this test is meaningless if 9000 km is representable"
    );

    let camera = Camera::new(
        FineTransform::new(far, FineRotation::IDENTITY),
        Frustum::DEFAULT,
    );
    let ray = camera.ray(CENTRE, SQUARE);

    assert_ne!(
        ray.origin,
        GlobalPoint::ZERO,
        "the ray fell back to the origin"
    );
    assert_eq!(
        ray.origin.x(),
        I24F8::MAX,
        "the x axis did not clamp to the edge"
    );
    assert_eq!(
        ray.origin.y(),
        I24F8::ZERO,
        "an in-range axis was disturbed"
    );
    assert_eq!(
        ray.origin.z(),
        I24F8::ZERO,
        "an in-range axis was disturbed"
    );

    // A camera inside the range is unaffected, which is the ordinary case.
    let near = globalfinepoint(1_000, 2_000, 3_000);
    let ordinary = Camera::new(
        FineTransform::new(near, FineRotation::IDENTITY),
        Frustum::DEFAULT,
    );
    assert_eq!(
        ordinary.ray(CENTRE, SQUARE).origin,
        near.to_global().unwrap()
    );
}

/// The direction is total: no screen position, however extreme the frustum,
/// leaves `normalize` with a zero vector to answer for.
///
/// The forward component is one whatever the other two saturate to, which is
/// the claim the fallback in `ray` rests on -- so it is checked rather than
/// asserted in a comment.
#[test]
fn every_corner_of_every_frustum_resolves_to_a_direction() {
    let wide = Frustum {
        slope: I16F16::MAX,
        ..Frustum::default()
    };
    let camera = Camera::new(FineTransform::default(), wide);

    for ndc in [
        (Signed16::MIN, Signed16::MIN),
        (Signed16::MIN, Signed16::MAX),
        (Signed16::MAX, Signed16::MIN),
        (Signed16::MAX, Signed16::MAX),
        (Signed16::ZERO, Signed16::ZERO),
    ] {
        for aspect in [I16F16::MIN, I16F16::ONE, I16F16::MAX] {
            let ray = camera.ray(ndc, aspect);
            assert!(
                ray.direction.to_fine().length() > I16F16::ZERO,
                "{ndc:?} at aspect {aspect:?} gave a degenerate direction"
            );
        }
    }
}
