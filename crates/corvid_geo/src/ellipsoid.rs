//! The reference ellipsoid a geodetic position is stated against.

use corvid_fixed::{I48F16, Signed32};

use crate::arith::{round_div, scale_q48, square_q48};

/// A reference ellipsoid, held as the integer constants every conversion in
/// this crate needs.
///
/// The two that matter to a European reconstruction are [`Ellipsoid::WGS84`],
/// which lon/lat archives publish against, and [`Ellipsoid::GRS80`], which
/// RGF93 and therefore Lambert-93 are defined on. They share an equatorial
/// radius exactly and their polar radii differ by a tenth of a millimetre, so
/// which one a position is stated against never changes where a building
/// stands -- but naming it is what makes that a fact rather than a hope.
///
/// The fields are private because their scales are, and the accessors answer
/// in metres: [`semi_major`](Self::semi_major) and
/// [`semi_minor`](Self::semi_minor).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ellipsoid {
    /// The equatorial radius, in Q16 metres.
    semi_major: i64,
    /// The polar radius, in Q16 metres.
    semi_minor: i64,
    /// `e^2 = f(2 - f)`, in Q48.
    eccentricity_squared: i64,
    /// `e'^2 = e^2 / (1 - e^2)`, in Q48.
    second_eccentricity_squared: i64,
}

impl Ellipsoid {
    /// The ellipsoid GPS reports against, and the one a lon/lat archive means
    /// unless it says otherwise.
    ///
    /// `a = 6378137 m` exactly, `1/f = 298.257223563`.
    pub const WGS84: Self = Self {
        semi_major: 417_997_586_432,
        semi_minor: 416_596_119_666,
        eccentricity_squared: 1_884_300_451_817,
        second_eccentricity_squared: 1_896_999_688_574,
    };

    /// The ellipsoid RGF93, and therefore EPSG:2154 Lambert-93, is defined on.
    ///
    /// `a = 6378137 m` exactly, `1/f = 298.257222101`.
    pub const GRS80: Self = Self {
        semi_major: 417_997_586_432,
        semi_minor: 416_596_119_660,
        eccentricity_squared: 1_884_300_461_038,
        second_eccentricity_squared: 1_896_999_697_919,
    };

    /// The equatorial radius, in metres.
    #[must_use]
    #[inline]
    pub const fn semi_major(self) -> I48F16 {
        I48F16::from_bits(self.semi_major)
    }

    /// The polar radius, in metres.
    #[must_use]
    #[inline]
    pub const fn semi_minor(self) -> I48F16 {
        I48F16::from_bits(self.semi_minor)
    }

    /// The equatorial radius as a Q16 bit pattern.
    pub(crate) const fn semi_major_bits(self) -> i64 {
        self.semi_major
    }

    /// The polar radius as a Q16 bit pattern.
    pub(crate) const fn semi_minor_bits(self) -> i64 {
        self.semi_minor
    }

    /// The prime vertical radius of curvature `N`, and `a * sqrt(1 - e^2
    /// sin^2 lat)` beside it, both in Q16 metres.
    ///
    /// The two come back together because they share a square root and because
    /// each conversion wants a different one of them: `N` places a point on
    /// the ellipsoid, and `a sqrt(W)` is what a height is measured above --
    /// `h = p cos(lat) + z sin(lat) - a sqrt(W)`, which is well conditioned at
    /// the poles where `p / cos(lat) - N` is not.
    ///
    /// `W = 1 - e^2 sin^2(lat)` never leaves `[0.9933, 1]`, so the root is
    /// taken by widening `W` from Q48 into Q96 and calling `isqrt`, which
    /// lands back in Q48 with no scaling step at all.
    pub(crate) const fn curvature(self, sin_lat: Signed32) -> (i64, i64) {
        let w = (1i128 << 48) - scale_q48(self.eccentricity_squared, square_q48(sin_lat)) as i128;
        let root = ((w as u128) << 48).isqrt() as i128;
        let prime_vertical = round_div((self.semi_major as i128) << 48, root) as i64;
        (prime_vertical, scale_q48(self.semi_major, root))
    }

    /// `N (1 - e^2)`, the radius the polar axis of an ECEF position is scaled
    /// by, in Q16 metres.
    pub(crate) const fn flattened(self, prime_vertical: i64) -> i64 {
        prime_vertical - scale_q48(prime_vertical, self.eccentricity_squared as i128)
    }

    /// `e'^2 b`, in Q16 metres: the coefficient of `sin^3` in Bowring's
    /// numerator.
    pub(crate) const fn bowring_rise(self) -> i64 {
        scale_q48(self.semi_minor, self.second_eccentricity_squared as i128)
    }

    /// `e^2 a`, in Q16 metres: the coefficient of `cos^3` in Bowring's
    /// denominator.
    pub(crate) const fn bowring_reach(self) -> i64 {
        scale_q48(self.semi_major, self.eccentricity_squared as i128)
    }

    /// The equatorial radius and `e^2`, as the bake-time projections want
    /// them.
    ///
    /// Derived from the same integer constants the tick-time half uses, so
    /// there is one definition of WGS84 in this crate rather than two that can
    /// drift apart. The Q48 eccentricity carries 48 bits and an `f64` mantissa
    /// 53, so widening it loses nothing.
    #[cfg(feature = "project")]
    #[must_use]
    pub fn f64_parts(self) -> (f64, f64) {
        (
            self.semi_major().to_f64(),
            self.eccentricity_squared as f64 / (1u64 << 48) as f64,
        )
    }
}
