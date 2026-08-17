//! A position on the ellipsoid, and the integer-only bridge to ECEF.

use corvid_fixed::{Angle32, I24F8, I48F16, Pitch32};
use corvid_vector::{GlobalFinePoint, GlobalPoint};

use crate::Ellipsoid;
use crate::arith::{cube_q48, fit_ratio, q16_to_q8, scale_by, scale_q48};

/// A latitude, a longitude and a height above the ellipsoid.
///
/// The two angles are the fixed-point types whose *semantics* already match
/// what they hold: a [`Pitch32`] clamps at the poles and an [`Angle32`] wraps
/// at the antimeridian, so no latitude and no longitude is ever out of range
/// and neither conversion needs a validity check. Both carry `2^-32` of a
/// turn, which is `8.4e-8` degrees, or **9.3 mm of northing** and 9.3 mm of
/// easting at the equator -- 6.1 mm of easting at the latitude of Paris. That
/// is the floor on everything here, and it is coarser than
/// [`GlobalPoint`]'s own 3.9 mm.
///
/// The height is an [`I24F8`], the same 3.9 mm as a world position, which
/// covers anything from the bottom of a cellar to well past low orbit.
///
/// ```
/// use corvid_fixed::{Angle32, I24F8, I48F16, Pitch32};
/// use corvid_geo::{Ellipsoid, Geodetic};
///
/// // La Folie Titon, faubourg Saint-Antoine.
/// let titon = Geodetic::new(
///     Pitch32::from_degrees(48.8524),
///     Angle32::from_degrees(2.3855),
///     I24F8::from_f64(35.0),
/// );
///
/// let ecef = titon.to_ecef(Ellipsoid::WGS84).expect("Paris is inside the world");
/// let back = Geodetic::from_ecef(ecef, Ellipsoid::WGS84);
///
/// // A round trip lands on the same angle to within a step or two of what a
/// // 32-bit angle can hold.
/// assert!(back.latitude().to_bits().abs_diff(titon.latitude().to_bits()) <= 2);
/// assert!(back.longitude().to_bits().abs_diff(titon.longitude().to_bits()) <= 2);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Geodetic {
    latitude: Pitch32,
    longitude: Angle32,
    height: I24F8,
}

impl Geodetic {
    /// A position, from its three parts.
    #[must_use]
    #[inline]
    pub const fn new(latitude: Pitch32, longitude: Angle32, height: I24F8) -> Self {
        Self {
            latitude,
            longitude,
            height,
        }
    }

    /// The latitude, positive north.
    #[must_use]
    #[inline]
    pub const fn latitude(self) -> Pitch32 {
        self.latitude
    }

    /// The longitude, positive east.
    #[must_use]
    #[inline]
    pub const fn longitude(self) -> Angle32 {
        self.longitude
    }

    /// The height above the ellipsoid, in metres.
    ///
    /// Above the *ellipsoid*, which is not above the sea: the geoid separation
    /// at Paris is about 44 m, and correcting for it is a caller's business
    /// because it needs a geoid model this crate does not carry.
    #[must_use]
    #[inline]
    pub const fn height(self) -> I24F8 {
        self.height
    }

    /// The same position at a different height.
    #[must_use]
    #[inline]
    pub const fn with_height(self, height: I24F8) -> Self {
        Self { height, ..self }
    }

    /// The earth-centred, earth-fixed position, at
    /// [`GlobalFinePoint`]'s 15.26 um.
    ///
    /// Total: the widest position this can answer is about `1.5e7` metres,
    /// which `I48F16` holds with eight orders of magnitude to spare. Use it
    /// where a level's anchor is being established, where the 3.9 mm of
    /// [`to_ecef`](Self::to_ecef) would be a millimetre of the answer rather
    /// than a millimetre of the last digit.
    ///
    /// The arithmetic is the textbook forward conversion, `p = (N + h)
    /// cos(lat)` and `z = (N (1 - e^2) + h) sin(lat)`, with every sine and
    /// cosine coming from [`Pitch32::sin_cos`] and [`Angle32::sin_cos`] --
    /// integer-only, correctly rounded, and identical on every machine. The
    /// last bit of a [`Signed32`](corvid_fixed::Signed32) sine is `4.7e-10`,
    /// which at the earth's radius is 3 mm, so two of them plus the rounding
    /// puts this within a centimetre of the same formula evaluated in `f64`.
    #[must_use]
    pub fn to_ecef_fine(self, ellipsoid: Ellipsoid) -> GlobalFinePoint {
        let (sin_lat, cos_lat) = self.latitude.sin_cos();
        let (sin_lon, cos_lon) = self.longitude.sin_cos();
        let (prime_vertical, _) = ellipsoid.curvature(sin_lat);
        let height = (self.height.to_bits() as i64) << 8;

        let equatorial = scale_by(prime_vertical + height, cos_lat);
        let polar = scale_by(ellipsoid.flattened(prime_vertical) + height, sin_lat);

        GlobalFinePoint::new(
            I48F16::from_bits(scale_by(equatorial, cos_lon)),
            I48F16::from_bits(scale_by(equatorial, sin_lon)),
            I48F16::from_bits(polar),
        )
    }

    /// The earth-centred, earth-fixed position a simulation holds, or `None`
    /// when it does not fit.
    ///
    /// `None` is a *range* answer and only a range answer: [`GlobalPoint`]
    /// reaches 8388 km an axis, so every point from the centre of the earth to
    /// about 2000 km above the surface converts, and nothing between them
    /// fails. A saturated axis would be a position on the wrong bearing rather
    /// than a position too far away, which is why this refuses instead.
    #[must_use]
    pub fn to_ecef(self, ellipsoid: Ellipsoid) -> Option<GlobalPoint> {
        self.to_ecef_fine(ellipsoid).to_global()
    }

    /// The geodetic position of an ECEF point, by Bowring's method.
    ///
    /// Total, and closed form: one arctangent for the auxiliary angle, one for
    /// the latitude, and no iteration at all. Bowring's approximation is worth
    /// under a micron of latitude for anything within a few hundred kilometres
    /// of the surface, which is far below the 9.3 mm a [`Pitch32`] can express,
    /// so iterating would refine a number that has nowhere to put the
    /// refinement.
    ///
    /// The height comes back as `p cos(lat) + z sin(lat) - a sqrt(W)` rather
    /// than as `p / cos(lat) - N`, because the second form divides by a cosine
    /// that is zero at the poles. It saturates past `I24F8`'s 8388 km, which
    /// no input can reach: the furthest corner of [`GlobalPoint`]'s box is
    /// 14532 km from the centre and the polar radius is 6357 km.
    #[must_use]
    pub fn from_ecef(point: GlobalPoint, ellipsoid: Ellipsoid) -> Self {
        let x = (point.x().to_bits() as i64) << 8;
        let y = (point.y().to_bits() as i64) << 8;
        let z = (point.z().to_bits() as i64) << 8;

        let longitude = Angle32::atan2(y, x);
        let equatorial = ((x as i128 * x as i128 + y as i128 * y as i128) as u128).isqrt() as i64;

        // The auxiliary angle, `atan(z a / p b)`. Only the ratio matters, so
        // both halves are shifted together until they fit the arctangent.
        let (rise, reach) = fit_ratio(
            z as i128 * ellipsoid.semi_major_bits() as i128,
            equatorial as i128 * ellipsoid.semi_minor_bits() as i128,
        );
        let (sin_aux, cos_aux) = Pitch32::atan2(rise, reach).sin_cos();

        let numerator = z + scale_q48(ellipsoid.bowring_rise(), cube_q48(sin_aux));
        let denominator = equatorial - scale_q48(ellipsoid.bowring_reach(), cube_q48(cos_aux));
        let latitude = Pitch32::atan2(numerator, denominator);

        let (sin_lat, cos_lat) = latitude.sin_cos();
        let (_, ellipsoid_radius) = ellipsoid.curvature(sin_lat);
        let height = scale_by(equatorial, cos_lat) + scale_by(z, sin_lat) - ellipsoid_radius;

        Self {
            latitude,
            longitude,
            height: I24F8::saturating_from_bits(q16_to_q8(height)),
        }
    }
}
