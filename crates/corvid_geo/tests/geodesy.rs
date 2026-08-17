//! The integer path against the floating-point path, on real coordinates.
//!
//! Paris is the subject because it is what this crate was written for: la
//! Folie Titon at 2.3855 E, 48.8524 N, and the four corners of the city's
//! bounding box around it.
#![allow(
    clippy::expect_used,
    reason = "a failed expect in a test is a failed test, which is what a test is for"
)]

use corvid_fixed::{Angle32, I24F8, Pitch32};
use corvid_geo::{Ellipsoid, Geodetic};

/// La Folie Titon and the corners of Paris, as degrees east and north.
const PARIS: [(f64, f64); 5] = [
    (2.3855, 48.8524),
    (2.2241, 48.8156),
    (2.4699, 48.8156),
    (2.2241, 48.9022),
    (2.4699, 48.9022),
];

fn at(longitude: f64, latitude: f64, height: f64) -> Geodetic {
    Geodetic::new(
        Pitch32::from_degrees(latitude),
        Angle32::from_degrees(longitude),
        I24F8::from_f64(height),
    )
}

#[test]
fn bowring_inverts_the_forward_conversion_over_paris() {
    for (longitude, latitude) in PARIS {
        for height in [-30.0, 0.0, 35.0, 300.0] {
            let position = at(longitude, latitude, height);
            let ecef = position
                .to_ecef(Ellipsoid::WGS84)
                .expect("Paris is inside the world");
            let back = Geodetic::from_ecef(ecef, Ellipsoid::WGS84);

            let north = back
                .latitude()
                .to_bits()
                .abs_diff(position.latitude().to_bits());
            let east = back
                .longitude()
                .to_bits()
                .wrapping_sub(position.longitude().to_bits())
                .cast_signed()
                .unsigned_abs();

            // One step of a `Pitch32` is 9.3 mm and one narrowing into
            // `GlobalPoint` is 3.9 mm an axis, so the round trip is allowed a
            // couple of steps and no more.
            assert!(north <= 2, "latitude drifted {north} steps at {latitude}");
            assert!(east <= 2, "longitude drifted {east} steps at {longitude}");
            assert!(
                (back.height().to_f64() - height).abs() <= 0.005,
                "height drifted to {} from {height}",
                back.height().to_f64()
            );
        }
    }
}

#[test]
fn a_height_comes_back_within_one_step_of_where_it_went_in() {
    // One step of an `I24F8` is 3.9 mm, and that is the entire budget. A
    // narrowing that shifted rather than divided would floor, so a cellar
    // would come back a step deeper than a cornice of the same size comes
    // back high -- which is what the signs in the inner loop are looking for.
    for latitude in [-89.0, -45.0, 0.0, 48.8524, 89.0] {
        for height in [0.5, 3.25, 30.0, 122.75, 1000.125] {
            for signed in [height, -height] {
                let position = at(2.3855, latitude, signed);
                let back = Geodetic::from_ecef(
                    position
                        .to_ecef(Ellipsoid::WGS84)
                        .expect("the ellipsoid is inside the world"),
                    Ellipsoid::WGS84,
                );

                let drift = back.height().to_f64() - signed;
                assert!(drift.abs() <= 0.004, "{drift} m at {latitude}, {signed}");
            }
        }
    }
}

#[test]
fn a_height_is_measured_along_the_normal_and_not_along_the_radius() {
    // The two differ by 11 arcminutes at this latitude, so a conversion that
    // confused them would put a 1000 m tower 3 m off its own footing.
    let ground = at(2.3855, 48.8524, 0.0);
    let tower = ground.with_height(I24F8::from_f64(1000.0));

    let low = ground.to_ecef(Ellipsoid::WGS84).expect("Paris");
    let high = tower.to_ecef(Ellipsoid::WGS84).expect("Paris");

    assert!((high.distance(low).to_f64() - 1000.0).abs() < 0.02);
    assert_eq!(
        Geodetic::from_ecef(high, Ellipsoid::WGS84)
            .latitude()
            .to_bits(),
        Geodetic::from_ecef(low, Ellipsoid::WGS84)
            .latitude()
            .to_bits(),
    );
}

#[test]
fn the_poles_and_the_equator_are_where_they_should_be() {
    let north = at(0.0, 90.0, 0.0)
        .to_ecef(Ellipsoid::WGS84)
        .expect("the pole is on the earth");
    assert!(north.x().to_f64().abs() < 0.01 && north.y().to_f64().abs() < 0.01);
    assert!((north.z().to_f64() - Ellipsoid::WGS84.semi_minor().to_f64()).abs() < 0.01);

    let greenwich = at(0.0, 0.0, 0.0)
        .to_ecef(Ellipsoid::WGS84)
        .expect("null island is on the earth");
    assert!((greenwich.x().to_f64() - Ellipsoid::WGS84.semi_major().to_f64()).abs() < 0.01);
    assert!(greenwich.y().to_f64().abs() < 0.01 && greenwich.z().to_f64().abs() < 0.01);
}

#[cfg(feature = "project")]
mod against_floating_point {
    use super::{PARIS, at};
    use corvid_geo::{ConformalConic, Ellipsoid, Wgs84};

    /// The largest disagreement, in metres, between the integer forward
    /// conversion and the same formula in `f64` on the same quantized angles.
    fn disagreement(longitude: f64, latitude: f64, height: f64) -> f64 {
        let position = at(longitude, latitude, height);
        let integer = position
            .to_ecef(Ellipsoid::WGS84)
            .expect("Paris is inside the world");
        let exact = Wgs84::from_geodetic(position).to_ecef(Ellipsoid::WGS84);

        [
            integer.x().to_f64() - exact[0],
            integer.y().to_f64() - exact[1],
            integer.z().to_f64() - exact[2],
        ]
        .into_iter()
        .fold(0.0_f64, |worst, axis| worst.max(axis.abs()))
    }

    #[test]
    fn the_integer_path_never_leaves_the_representation_behind() {
        for (longitude, latitude) in PARIS {
            let worst = disagreement(longitude, latitude, 35.0);
            // Three millimetres per sine, twice, plus a 3.9 mm grid to land
            // on. A centimetre is the sum of what the types can hold, not a
            // tolerance chosen to make the test pass.
            assert!(worst < 0.01, "{worst} m at {longitude}, {latitude}");
        }
    }

    #[test]
    fn the_integer_path_holds_over_the_whole_ellipsoid() {
        for latitude in [-89.0, -45.0, 0.0, 12.5, 45.0, 89.0] {
            for longitude in [-179.0, -90.0, 0.0, 2.3855, 90.0, 179.0] {
                let worst = disagreement(longitude, latitude, 0.0);
                assert!(worst < 0.01, "{worst} m at {longitude}, {latitude}");
            }
        }
    }

    #[test]
    fn lambert93_round_trips_through_wgs84_and_ecef() {
        for (longitude, latitude) in PARIS {
            let start = Wgs84::new(longitude, latitude, 35.0);

            let grid = ConformalConic::LAMBERT93.forward(start);
            let unprojected = ConformalConic::LAMBERT93.inverse(grid);
            let ecef = unprojected.to_ecef(Ellipsoid::GRS80);
            let back = Wgs84::from_ecef(ecef, Ellipsoid::GRS80);

            // A degree of latitude is 111 km, so 1e-8 degrees is a millimetre.
            assert!(
                (back.latitude() - latitude).abs() < 1e-8,
                "latitude came back {} from {latitude}",
                back.latitude()
            );
            assert!(
                (back.longitude() - longitude).abs() < 1e-8,
                "longitude came back {} from {longitude}",
                back.longitude()
            );
            assert!((back.height() - 35.0).abs() < 1e-3);
        }
    }

    #[test]
    fn lambert93_puts_paris_where_the_national_grid_says_it_is() {
        // Paris sits near 652 km east and 6862 km north on EPSG:2154; the
        // check is loose because its point is that the parameters are the
        // right ones, not that this reproduces a particular survey.
        let grid = ConformalConic::LAMBERT93.forward(Wgs84::new(2.3855, 48.8524, 0.0));

        assert!(
            (640_000.0..670_000.0).contains(&grid.easting),
            "easting {}",
            grid.easting
        );
        assert!(
            (6_850_000.0..6_875_000.0).contains(&grid.northing),
            "northing {}",
            grid.northing
        );
    }

    #[test]
    fn the_grid_origin_is_the_false_origin() {
        let grid = ConformalConic::LAMBERT93.forward(Wgs84::new(3.0, 46.5, 0.0));

        assert!((grid.easting - 700_000.0).abs() < 1e-6);
        assert!((grid.northing - 6_600_000.0).abs() < 1e-6);
    }

    #[test]
    fn the_standard_parallels_are_where_the_scale_is_true() {
        // A conformal conic is exact on its two standard parallels: a short
        // step of longitude there measures its true length on the grid. Short
        // because the grid distance is a chord and the earth distance an arc,
        // and over a whole degree the two differ by more than the scale error
        // being looked for.
        let step = 0.01;
        for latitude in [44.0, 49.0] {
            let west = ConformalConic::LAMBERT93.forward(Wgs84::new(3.0, latitude, 0.0));
            let east = ConformalConic::LAMBERT93.forward(Wgs84::new(3.0 + step, latitude, 0.0));
            let on_grid = (east.easting - west.easting).hypot(east.northing - west.northing);

            let (semi_major, eccentricity_squared) = Ellipsoid::GRS80.f64_parts();
            let sin = latitude.to_radians().sin();
            let on_earth = semi_major * latitude.to_radians().cos()
                / (1.0 - eccentricity_squared * sin * sin).sqrt()
                * step.to_radians();

            assert!(
                (on_grid - on_earth).abs() < 1e-3,
                "{on_grid} against {on_earth} at {latitude}"
            );
        }
    }

    #[test]
    fn the_closed_form_inverse_agrees_with_the_forward_conversion() {
        for (longitude, latitude) in PARIS {
            for height in [-400.0, 0.0, 35.0, 8_848.0, 400_000.0] {
                let start = Wgs84::new(longitude, latitude, height);
                let back = Wgs84::from_ecef(start.to_ecef(Ellipsoid::WGS84), Ellipsoid::WGS84);

                assert!((back.latitude() - latitude).abs() < 1e-11);
                assert!((back.longitude() - longitude).abs() < 1e-11);
                assert!((back.height() - height).abs() < 1e-6, "height {height}");
            }
        }
    }
}
