//! Lambert Conformal Conic with two standard parallels, EPSG method 9802.
//!
//! The projection France publishes in. Everything here is `f64` and everything
//! here is bake time: a logarithm and a power are what a conformal conic is
//! made of, and neither belongs in a tick. What a level stores is the
//! [`Geodetic`](crate::Geodetic) this produced, in fixed point.

use core::f64::consts::{FRAC_PI_2, FRAC_PI_4};

use crate::Ellipsoid;
use crate::project::Wgs84;

/// A grid reference: metres east and north on a projected plane, with the
/// ellipsoidal height carried through untouched.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Projected {
    /// Metres east on the grid, false easting included.
    pub easting: f64,
    /// Metres north on the grid, false northing included.
    pub northing: f64,
    /// Metres above the ellipsoid. A conic projection is two dimensional and
    /// leaves this alone; it rides along so a round trip is a round trip.
    pub height: f64,
}

/// A Lambert conformal conic projection, by its defining parameters.
///
/// [`ConformalConic::LAMBERT93`] is EPSG:2154, which is what every French
/// national dataset since 2001 is published in -- the cadastre, the BD TOPO,
/// the IGN scans. Its parameters are the ones the EPSG registry states, on
/// GRS80, and they are written out on the constant rather than hidden in a
/// table so that a reader can check them against the registry.
///
/// **No datum shift is applied and none is needed.** RGF93 is the French
/// realization of ETRS89 and agrees with WGS84 to within a couple of
/// centimetres at the epochs an archive is georeferenced at, which is finer
/// than [`Geodetic`](crate::Geodetic) can express. A projection between two
/// datums that genuinely differ -- NTF and its Paris meridian, say -- needs a
/// seven-parameter transformation this type does not carry.
///
/// ```
/// use corvid_geo::{ConformalConic, Projected, Wgs84};
///
/// // La Folie Titon in Lambert-93, and back.
/// let titon = Wgs84::new(2.3855, 48.8524, 35.0);
/// let grid = ConformalConic::LAMBERT93.forward(titon);
/// let back = ConformalConic::LAMBERT93.inverse(grid);
///
/// assert!((grid.easting - 654_000.0).abs() < 2_000.0);
/// assert!((back.longitude() - titon.longitude()).abs() < 1e-11);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConformalConic {
    ellipsoid: Ellipsoid,
    first_parallel: f64,
    second_parallel: f64,
    origin_latitude: f64,
    central_meridian: f64,
    false_easting: f64,
    false_northing: f64,
}

impl ConformalConic {
    /// EPSG:2154, RGF93 / Lambert-93: the French national grid.
    pub const LAMBERT93: Self = Self {
        ellipsoid: Ellipsoid::GRS80,
        first_parallel: 44.0,
        second_parallel: 49.0,
        origin_latitude: 46.5,
        central_meridian: 3.0,
        false_easting: 700_000.0,
        false_northing: 6_600_000.0,
    };

    /// A projection from its parameters: the two standard parallels, the
    /// latitude and longitude of the origin, and the false easting and
    /// northing, all in degrees and metres.
    #[must_use]
    #[inline]
    pub const fn new(
        ellipsoid: Ellipsoid,
        first_parallel: f64,
        second_parallel: f64,
        origin_latitude: f64,
        central_meridian: f64,
        false_easting: f64,
        false_northing: f64,
    ) -> Self {
        Self {
            ellipsoid,
            first_parallel,
            second_parallel,
            origin_latitude,
            central_meridian,
            false_easting,
            false_northing,
        }
    }

    /// The ellipsoid the grid is defined on.
    #[must_use]
    #[inline]
    pub const fn ellipsoid(self) -> Ellipsoid {
        self.ellipsoid
    }

    /// A geodetic position placed on the grid.
    #[must_use]
    pub fn forward(self, position: Wgs84) -> Projected {
        let shape = Shape::of(self);
        let radius = shape.radius(position.latitude().to_radians());
        let angle = shape.cone * (position.longitude() - self.central_meridian).to_radians();

        Projected {
            easting: self.false_easting + radius * angle.sin(),
            northing: self.false_northing + shape.origin_radius - radius * angle.cos(),
            height: position.height(),
        }
    }

    /// A grid reference read back as a geodetic position.
    ///
    /// The latitude comes out of a fixed-point iteration on the isometric
    /// latitude, which converges quadratically and is run to `1e-14` radians
    /// -- a nanometre on the ground, and eight orders finer than anything this
    /// crate stores.
    #[must_use]
    pub fn inverse(self, grid: Projected) -> Wgs84 {
        let shape = Shape::of(self);
        let east = grid.easting - self.false_easting;
        let north = shape.origin_radius - (grid.northing - self.false_northing);

        // The cone's sign decides which way the developed plane runs, and both
        // the radius and the bearing carry it. Without it a southern-hemisphere
        // grid reads as its own mirror image.
        let sign = if shape.cone < 0.0 { -1.0 } else { 1.0 };
        let radius = sign * (east * east + north * north).sqrt();
        let angle = (sign * east).atan2(sign * north);
        let isometric = (radius / (shape.scale * shape.equatorial)).powf(1.0 / shape.cone);

        let mut latitude = FRAC_PI_2 - 2.0 * isometric.atan();
        for _ in 0..12 {
            let sine = shape.eccentricity * latitude.sin();
            let next = FRAC_PI_2
                - 2.0
                    * (isometric * ((1.0 - sine) / (1.0 + sine)).powf(shape.eccentricity / 2.0))
                        .atan();
            let settled = (next - latitude).abs() < 1e-14;
            latitude = next;
            if settled {
                break;
            }
        }

        Wgs84::new(
            (angle / shape.cone).to_degrees() + self.central_meridian,
            latitude.to_degrees(),
            grid.height,
        )
    }
}

/// The three constants a conic's parameters reduce to.
///
/// Recomputed per call rather than cached in [`ConformalConic`], because a
/// projection runs once per feature at bake time and a cache is a second copy
/// of the parameters that can disagree with the first.
#[derive(Clone, Copy, Debug)]
struct Shape {
    equatorial: f64,
    eccentricity: f64,
    /// `n`, the cone constant: the fraction of a full turn the developed cone
    /// covers.
    cone: f64,
    /// `F`, the scale the radius is measured in.
    scale: f64,
    /// `r0`, the radius of the latitude of origin.
    origin_radius: f64,
}

impl Shape {
    fn of(conic: ConformalConic) -> Self {
        let (equatorial, eccentricity_squared) = conic.ellipsoid.f64_parts();
        let eccentricity = eccentricity_squared.sqrt();

        let first = conic.first_parallel.to_radians();
        let second = conic.second_parallel.to_radians();
        let first_scale = parallel_scale(first, eccentricity);
        let second_scale = parallel_scale(second, eccentricity);
        let first_isometric = isometric(first, eccentricity);
        let second_isometric = isometric(second, eccentricity);

        // Two standard parallels at the same latitude would be the one-parallel
        // case, which has a different formula rather than a degenerate one.
        let cone = if (first_isometric - second_isometric).abs() < f64::EPSILON {
            first.sin()
        } else {
            (first_scale.ln() - second_scale.ln()) / (first_isometric.ln() - second_isometric.ln())
        };
        let scale = first_scale / (cone * first_isometric.powf(cone));
        let origin_radius = equatorial
            * scale
            * isometric(conic.origin_latitude.to_radians(), eccentricity).powf(cone);

        Self {
            equatorial,
            eccentricity,
            cone,
            scale,
            origin_radius,
        }
    }

    /// `r`, the radius of one latitude on the developed cone.
    fn radius(self, latitude: f64) -> f64 {
        self.equatorial * self.scale * isometric(latitude, self.eccentricity).powf(self.cone)
    }
}

/// `m`, the radius of the parallel at `latitude` relative to the equatorial
/// radius.
fn parallel_scale(latitude: f64, eccentricity: f64) -> f64 {
    let sin = eccentricity * latitude.sin();
    latitude.cos() / (1.0 - sin * sin).sqrt()
}

/// `t`, the isometric parameter: the conformal latitude written so that the
/// projection is a power of it.
fn isometric(latitude: f64, eccentricity: f64) -> f64 {
    let sin = eccentricity * latitude.sin();
    (FRAC_PI_4 - latitude / 2.0).tan() / ((1.0 - sin) / (1.0 + sin)).powf(eccentricity / 2.0)
}
