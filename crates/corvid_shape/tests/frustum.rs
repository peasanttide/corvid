//! What a view keeps and what it culls.
//!
//! Culling is the one place in this crate where the two possible mistakes are
//! not equal. Drawing something invisible costs a frame's work; omitting
//! something visible is a hole in the picture. So every test here that fixes a
//! bound checks the *inclusive* side of it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_fixed::{Angle16, I16F16};
use corvid_shape::Frustum;
use corvid_vector::finepoint;

/// A ninety-degree view, near at a tenth of a metre and far at a kilometre.
const fn lens() -> Frustum {
    Frustum::perspective(
        Angle16::from_degrees(90.0),
        I16F16::from_f64(0.1),
        I16F16::from_f64(1000.0),
    )
}

/// Ninety degrees vertically is a slope of one: as high as it is far.
#[test]
fn a_right_angle_is_a_slope_of_one() {
    assert!((lens().slope.to_f64() - 1.0).abs() < 1e-4);
}

/// A sphere reaching over a leaning side plane is kept.
///
/// The centre is outside the side plane in `z` and closer to the *plane* than
/// its radius, which is what "partly inside" means for a plane that leans. A
/// bound that inflated the half-height straight up rather than along the
/// plane's normal culled this, which is the one answer the documentation
/// promises never to give.
///
/// At slope one the plane's normal is `(1, -1)/sqrt(2)`, so the centre at
/// `z = 11.2` with the half-height at 10 is `1.2 / sqrt(2)`, about 0.85, from
/// the plane -- inside a radius of one.
#[test]
fn a_sphere_reaching_over_a_leaning_side_is_kept() {
    let centre = finepoint(I16F16::ZERO, I16F16::from_f64(10.0), I16F16::from_f64(11.2));
    assert!(
        lens().intersects_sphere(centre, I16F16::from_f64(1.0), I16F16::ONE),
        "a sphere 0.85 from the side plane was culled by a radius of one",
    );
}

/// One far enough out to be genuinely outside is still culled.
///
/// The companion to the test above: an inflation generous enough to keep
/// everything would satisfy that one and say nothing.
#[test]
fn a_sphere_clear_of_the_side_is_culled() {
    let centre = finepoint(I16F16::ZERO, I16F16::from_f64(10.0), I16F16::from_f64(40.0));
    assert!(!lens().intersects_sphere(centre, I16F16::from_f64(1.0), I16F16::ONE));
}

/// A negative radius is an empty sphere here, as it is everywhere else.
///
/// `Sphere::contains` and casting at one both read a negative radius as
/// holding no points. A culler that kept it would be the one place in the
/// crate where an empty ball is visible.
#[test]
fn a_negative_radius_is_empty_even_at_the_centre_of_the_view() {
    let centre = finepoint(I16F16::ZERO, I16F16::from_f64(10.0), I16F16::ZERO);
    assert!(!lens().intersects_sphere(centre, I16F16::from_f64(-1.0), I16F16::ONE));
}

/// A field of view with no half a pitch can hold saturates rather than
/// wrapping into a narrow view.
///
/// `Angle16::half` answers `None` past a half turn, and the slope that comes
/// back is the same saturation a quarter-turn half-angle already gives -- not
/// the small slope that halving-then-wrapping would have produced.
#[test]
fn a_field_of_view_past_a_half_turn_saturates() {
    let absurd = Frustum::perspective(
        Angle16::from_degrees(270.0),
        I16F16::from_f64(0.1),
        I16F16::from_f64(1000.0),
    );
    assert_eq!(absurd.slope, I16F16::MAX);
    assert_eq!(Angle16::from_degrees(270.0).half(), None);
}
