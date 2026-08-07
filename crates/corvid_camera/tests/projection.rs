//! What a screen position denotes, and which way a field of view widens.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_camera::{Camera, FirstPerson};
use corvid_fixed::{Angle16, Angle32, I16F16, Pitch32, Signed32};
use corvid_rotation::{FineRotation, Versor};
use corvid_shape::{Frustum, Ray};
use corvid_transform::GlobalFineTransform;
use corvid_vector::{Direction, GlobalFinePoint, globalfinepoint, globalpoint};

/// At the origin, facing forward, with the default frustum.
const fn eye() -> Camera {
    at(GlobalFineTransform::new(
        GlobalFinePoint::ZERO,
        FineRotation::IDENTITY,
    ))
}

/// A camera at a given pose, with the default frustum.
const fn at(pose: GlobalFineTransform) -> Camera {
    let mut camera = FirstPerson::new(pose.position().to_global().unwrap());
    camera.facing = pose.rotation();
    camera.camera()
}

/// The middle of the screen.
const CENTRE: (Signed32, Signed32) = (Signed32::ZERO, Signed32::ZERO);

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
    let pose = GlobalFineTransform::new(globalfinepoint(1, 2, 3), FineRotation::IDENTITY);
    let ray = at(pose).ray(CENTRE, SQUARE);
    assert_eq!(ray.origin, globalpoint(1, 2, 3));
}

/// Half the vertical field of view is what the top of the screen is at. At 60°
/// that is 30°, whose tangent is 0.5774 — so the ray's up over its forward is
/// that.
#[test]
fn the_top_edge_is_half_the_fov_up() {
    let top = eye().ray((Signed32::ZERO, Signed32::MAX), SQUARE);
    let rise = slope(top, 2);
    assert!((rise - 0.577_350).abs() < 1e-3, "{rise}");
}

/// And the bottom is the same the other way, exactly.
#[test]
fn the_screen_is_symmetric() {
    let top = eye().ray((Signed32::ZERO, Signed32::MAX), SQUARE);
    let bottom = eye().ray((Signed32::ZERO, Signed32::MIN), SQUARE);
    assert!((slope(top, 2) + slope(bottom, 2)).abs() < 1e-3);
}

/// `slope` is the tangent of half the field of view, which is the one
/// trigonometric quantity a projection has.
#[test]
fn the_slope_is_the_half_angle_tangent() {
    let ninety = Frustum::default().with_fov(Angle16::from_degrees(90.0));
    // tan 45° is one.
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
    let square = eye().ray((Signed32::MAX, Signed32::ZERO), SQUARE);
    let wide = eye().ray((Signed32::MAX, Signed32::ZERO), I16F16::from_f64(2.0));
    assert!(
        slope(wide, 0) > slope(square, 0) * 1.9,
        "{} vs {}",
        slope(wide, 0),
        slope(square, 0)
    );

    // And the vertical is untouched by it.
    let tall_square = eye().ray((Signed32::ZERO, Signed32::MAX), SQUARE);
    let tall_wide = eye().ray((Signed32::ZERO, Signed32::MAX), I16F16::from_f64(2.0));
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
    let pose = GlobalFineTransform::new(GlobalFinePoint::ZERO, quarter);
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
    let pose = GlobalFineTransform::new(GlobalFinePoint::ZERO, raised);
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
