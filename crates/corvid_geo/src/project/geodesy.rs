//! Bake-time geodesy in `f64`: lon/lat as an archive states it, and the exact
//! conversion to and from ECEF.

use corvid_fixed::{Angle32, I24F8, Pitch32};

use crate::{Ellipsoid, Geodetic};

/// A geodetic position in degrees and metres, as a lon/lat archive publishes
/// one.
///
/// **This type is floating point and therefore bake-time only.** It exists to
/// read a shapefile, a `GeoJSON` feature or a projected grid reference and turn
/// it into a [`Geodetic`], which is the integer type a tick may touch. Nothing
/// that reaches a hashed value may be computed here; see the crate front page
/// for why that line is drawn where it is.
///
/// ```
/// use corvid_geo::{Ellipsoid, Wgs84};
///
/// // La Folie Titon, the wallpaper works on the faubourg Saint-Antoine.
/// let titon = Wgs84::new(2.3855, 48.8524, 35.0);
/// let ecef = titon.to_ecef(Ellipsoid::WGS84);
/// let back = Wgs84::from_ecef(ecef, Ellipsoid::WGS84);
///
/// assert!((back.height() - titon.height()).abs() < 1e-6);
/// assert!((back.latitude() - titon.latitude()).abs() < 1e-12);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Wgs84 {
    longitude: f64,
    latitude: f64,
    height: f64,
}

impl Wgs84 {
    /// A position, from degrees east, degrees north and metres above the
    /// ellipsoid.
    #[must_use]
    #[inline]
    pub const fn new(longitude: f64, latitude: f64, height: f64) -> Self {
        Self {
            longitude,
            latitude,
            height,
        }
    }

    /// Degrees east of Greenwich.
    #[must_use]
    #[inline]
    pub const fn longitude(self) -> f64 {
        self.longitude
    }

    /// Degrees north of the equator.
    #[must_use]
    #[inline]
    pub const fn latitude(self) -> f64 {
        self.latitude
    }

    /// Metres above the ellipsoid.
    #[must_use]
    #[inline]
    pub const fn height(self) -> f64 {
        self.height
    }

    /// The fixed-point position a simulation may hold.
    ///
    /// This is the end of the floating-point half: everything after it is
    /// integers. The two angles quantize to `2^-32` of a turn, which is 9.3 mm
    /// on the ground, and the height to 3.9 mm.
    #[must_use]
    pub fn to_geodetic(self) -> Geodetic {
        Geodetic::new(
            Pitch32::from_degrees(self.latitude),
            Angle32::from_degrees(self.longitude),
            I24F8::from_f64(self.height),
        )
    }

    /// The same position read back out as degrees, for a report or a check.
    #[must_use]
    pub fn from_geodetic(position: Geodetic) -> Self {
        Self::new(
            position.longitude().to_signed_radians().to_degrees(),
            position.latitude().to_degrees(),
            position.height().to_f64(),
        )
    }

    /// The earth-centred, earth-fixed position, in metres.
    #[must_use]
    pub fn to_ecef(self, ellipsoid: Ellipsoid) -> [f64; 3] {
        let (semi_major, eccentricity_squared) = ellipsoid.f64_parts();
        let (sin_lat, cos_lat) = self.latitude.to_radians().sin_cos();
        let (sin_lon, cos_lon) = self.longitude.to_radians().sin_cos();

        let prime_vertical = semi_major / (1.0 - eccentricity_squared * sin_lat * sin_lat).sqrt();
        let equatorial = (prime_vertical + self.height) * cos_lat;

        [
            equatorial * cos_lon,
            equatorial * sin_lon,
            (prime_vertical * (1.0 - eccentricity_squared) + self.height) * sin_lat,
        ]
    }

    /// The geodetic position of an ECEF point, in closed form.
    ///
    /// Heikkinen's solution of the quartic Ferrari reduced: no iteration, and
    /// exact to the last bits of an `f64` everywhere outside a few kilometres
    /// of the earth's centre. That is why this half is worth having at all --
    /// the integer [`Geodetic::from_ecef`] answers Bowring's approximation,
    /// and the test that the two agree is the thing that says the
    /// approximation was allowed.
    #[must_use]
    pub fn from_ecef(ecef: [f64; 3], ellipsoid: Ellipsoid) -> Self {
        let [x, y, z] = ecef;
        let (semi_major, eccentricity_squared) = ellipsoid.f64_parts();
        let semi_minor = semi_major * (1.0 - eccentricity_squared).sqrt();
        let second = eccentricity_squared / (1.0 - eccentricity_squared);
        let quartic = eccentricity_squared * eccentricity_squared;

        let equatorial_squared = x * x + y * y;
        let equatorial = equatorial_squared.sqrt();
        let scaled = 54.0 * semi_minor * semi_minor * z * z;
        let sum = equatorial_squared + (1.0 - eccentricity_squared) * z * z
            - eccentricity_squared * (semi_major * semi_major - semi_minor * semi_minor);
        let ratio = quartic * scaled * equatorial_squared / (sum * sum * sum);
        let cube = (1.0 + ratio + (ratio * ratio + 2.0 * ratio).sqrt()).cbrt();
        let shell = cube + 1.0 / cube + 1.0;
        let part = scaled / (3.0 * shell * shell * sum * sum);
        let quotient = (1.0 + 2.0 * quartic * part).sqrt();

        let radical = semi_major * semi_major / 2.0 * (1.0 + 1.0 / quotient)
            - part * (1.0 - eccentricity_squared) * z * z / (quotient * (1.0 + quotient))
            - part * equatorial_squared / 2.0;
        let foot = -(part * eccentricity_squared * equatorial) / (1.0 + quotient)
            + radical.max(0.0).sqrt();
        let offset = equatorial - eccentricity_squared * foot;

        let chord = (offset * offset + z * z).sqrt();
        let flattened = (offset * offset + (1.0 - eccentricity_squared) * z * z).sqrt();
        let polar = semi_minor * semi_minor * z / (semi_major * flattened);

        Self::new(
            y.atan2(x).to_degrees(),
            ((z + second * polar) / equatorial).atan().to_degrees(),
            chord * (1.0 - semi_minor * semi_minor / (semi_major * flattened)),
        )
    }
}
