//! The local frame: that a level's metres are metres, and that they are the
//! right metres.
#![allow(
    clippy::expect_used,
    reason = "a failed expect in a test is a failed test, which is what a test is for"
)]

use corvid_fixed::{Angle32, I24F8, Pitch32, Signed32};
use corvid_geo::{Anchor, Ellipsoid, Geodetic};
use corvid_vector::{GlobalPoint, globalpoint};

/// La Folie Titon, at ground level.
fn titon() -> Geodetic {
    Geodetic::new(
        Pitch32::from_degrees(48.8524),
        Angle32::from_degrees(2.3855),
        I24F8::ZERO,
    )
}

#[test]
fn a_local_offset_survives_the_trip_to_the_earth_and_back() {
    let anchor = Anchor::new(titon(), Ellipsoid::WGS84);

    for offset in [
        GlobalPoint::ZERO,
        globalpoint(1, 0, 0),
        globalpoint(-250, 400, 30),
        globalpoint(5000, -5000, -20),
    ] {
        let world = anchor.to_ecef(offset).expect("Paris is on the earth");
        let back = anchor.to_local(world).expect("and it comes back");

        // Both halves round into `GlobalPoint`'s 3.9 mm grid, and a
        // projection onto three directions rounds once more.
        assert!(
            back.distance(offset) <= I24F8::from_f64(0.02),
            "{offset:?} came back as {back:?}"
        );
    }
}

#[test]
fn the_basis_is_orthonormal() {
    let anchor = Anchor::new(titon(), Ellipsoid::WGS84);
    let square = Signed32::from_f64(0.0001);

    assert!(anchor.east().align(anchor.north()).abs() < square);
    assert!(anchor.north().align(anchor.up()).abs() < square);
    assert!(anchor.up().align(anchor.east()).abs() < square);
}

#[test]
fn east_runs_along_the_parallel_and_never_leaves_it() {
    // Due east is perpendicular to the polar axis everywhere on the earth, so
    // its ECEF z component is zero exactly rather than nearly.
    for latitude in [-89.0, -48.85, 0.0, 48.85, 89.0] {
        let anchor = Anchor::new(
            Geodetic::new(
                Pitch32::from_degrees(latitude),
                Angle32::from_degrees(2.3855),
                I24F8::ZERO,
            ),
            Ellipsoid::WGS84,
        );

        assert_eq!(anchor.east().z(), Signed32::ZERO, "at {latitude}");
    }
}

#[test]
fn up_is_the_direction_a_height_is_measured_along() {
    let anchor = Anchor::new(titon(), Ellipsoid::WGS84);

    let raised = anchor
        .to_ecef(globalpoint(0, 0, 100))
        .expect("a hundred metres up is still on the earth");
    let above = Geodetic::from_ecef(raised, Ellipsoid::WGS84);

    assert!((above.height().to_f64() - 100.0).abs() < 0.02);
    assert!(
        above
            .latitude()
            .to_bits()
            .abs_diff(titon().latitude().to_bits())
            <= 2
    );
}

#[test]
fn a_kilometre_north_moves_the_latitude_by_what_a_kilometre_is() {
    let anchor = Anchor::new(titon(), Ellipsoid::WGS84);

    let north = anchor
        .to_ecef(globalpoint(0, 1000, 0))
        .expect("a kilometre north of the faubourg");
    let moved = Geodetic::from_ecef(north, Ellipsoid::WGS84);

    // A degree of latitude is 111.2 km at this latitude, so a kilometre is
    // 0.00899 degrees. The tangent plane rises 78 mm over a kilometre, which
    // is where the height goes.
    let step = moved.latitude().to_degrees() - titon().latitude().to_degrees();
    assert!((step - 0.008_99).abs() < 1e-4, "moved {step} degrees");
    assert!(moved.height().to_f64() > 0.0);
    assert!(moved.height().to_f64() < 0.1);
}

#[test]
fn the_anchor_remembers_where_it_was_put() {
    let anchor = Anchor::new(titon(), Ellipsoid::WGS84);

    assert_eq!(anchor.origin(), titon());
    assert_eq!(
        anchor.to_local(anchor.ecef().to_global().expect("Paris")),
        Some(GlobalPoint::ZERO)
    );
}
