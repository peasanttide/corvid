//! What a shader needs to draw a sky, and no shader.
//!
//! This crate has no opinion about how a sky is rendered. What it has is the
//! medium: the scattering and absorption coefficients of the air, the two
//! phase functions that say which way light goes when it hits it, the density
//! profile to march through, and the transmittance along a path to space. A
//! caller builds its own lookup tables out of those, or ray-marches them, or
//! evaluates an analytic approximation -- all three want the same numbers and
//! none of them belongs here.
//!
//! The coefficients are the ones tabulated in Hillaire, *A Scalable and
//! Production Ready Sky and Atmosphere Rendering Technique*, Eurographics
//! Symposium on Rendering 2020, which are Bruneton and Neyret's fitted to
//! three colour channels. **Everything here is in metres and per metre.**

use crate::math::sin;

/// Pi, at the precision an `f64` holds it.
const PI: f64 = core::f64::consts::PI;

/// The medium between an observer and space.
///
/// Three components, because three are what the eye sees. **Rayleigh**
/// scattering is molecular, goes as the inverse fourth power of wavelength,
/// and is why the sky is blue and the setting sun is red. **Mie** scattering
/// is aerosol -- dust, smoke, water -- is nearly wavelength-independent, is
/// strongly forward-scattering, and is the white haze around the sun and the
/// entire visible effect of a fire. **Ozone** does not scatter at all; it
/// absorbs in the green and orange, and it is the only reason a twilight sky
/// goes deep blue instead of muddy brown once the sun is well down.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Atmosphere {
    /// Rayleigh scattering coefficient at sea level, per metre, for red, green
    /// and blue.
    pub rayleigh_scattering: [f64; 3],
    /// The height over which Rayleigh density falls by a factor of `e`, in
    /// metres.
    pub rayleigh_height: f64,
    /// Mie scattering coefficient at sea level, per metre. Grey, so one
    /// number.
    pub mie_scattering: f64,
    /// Mie absorption coefficient at sea level, per metre.
    pub mie_absorption: f64,
    /// The height over which Mie density falls by a factor of `e`, in metres.
    /// Aerosol hugs the ground: this is about a seventh of the Rayleigh
    /// height.
    pub mie_height: f64,
    /// The Henyey-Greenstein asymmetry of Mie scattering, `-1.0 ..= 1.0`.
    /// Around 0.8, meaning strongly forward.
    pub mie_asymmetry: f64,
    /// Ozone absorption coefficient at the layer's peak, per metre, for red,
    /// green and blue.
    pub ozone_absorption: [f64; 3],
    /// The altitude of the peak of the ozone layer, in metres.
    pub ozone_centre: f64,
    /// Half the width of the triangular ozone layer, in metres.
    pub ozone_width: f64,
    /// The planet's radius, in metres.
    pub ground_radius: f64,
    /// The radius at which the atmosphere is treated as ending, in metres.
    pub top_radius: f64,
}

impl Atmosphere {
    /// A clean Earth atmosphere: Hillaire's table, unmodified.
    pub const EARTH: Self = Self {
        rayleigh_scattering: [5.802e-6, 13.558e-6, 33.1e-6],
        rayleigh_height: 8_000.0,
        mie_scattering: 3.996e-6,
        mie_absorption: 4.40e-6,
        mie_height: 1_200.0,
        mie_asymmetry: 0.8,
        ozone_absorption: [0.650e-6, 1.881e-6, 0.085e-6],
        ozone_centre: 25_000.0,
        ozone_width: 15_000.0,
        ground_radius: 6_360_000.0,
        top_radius: 6_460_000.0,
    };

    /// The same atmosphere with its aerosol scaled.
    ///
    /// One is Hillaire's clean air. This is the one knob a game is expected to
    /// drive from its own simulation: a burning building puts a smoke column
    /// into the air, and a smoke column is aerosol. Turning this up reddens the
    /// sun, whitens the sky near it, and cuts the transmittance, which is the
    /// difference between a plume that is a term in the sky and a plume that is
    /// a decal drawn over it.
    #[must_use]
    pub const fn with_aerosol(mut self, turbidity: f64) -> Self {
        self.mie_scattering *= turbidity;
        self.mie_absorption *= turbidity;
        self
    }

    /// The three densities at a height above the ground, each `0.0 ..= 1.0`
    /// relative to its own sea-level or peak value: Rayleigh, Mie, ozone.
    ///
    /// This is what a ray march multiplies the coefficients by, sample by
    /// sample. The first two are exponential and the third is a triangle,
    /// because ozone is a layer and not an atmosphere.
    #[must_use]
    pub fn density(&self, height: f64) -> [f64; 3] {
        let ozone = 1.0 - libm::fabs(height - self.ozone_centre) / self.ozone_width;
        [
            libm::exp(-height / self.rayleigh_height),
            libm::exp(-height / self.mie_height),
            ozone.max(0.0),
        ]
    }

    /// The optical depth straight up, per channel: the integral of every
    /// coefficient through the whole column.
    ///
    /// Analytic, because two exponentials and a triangle integrate. The
    /// Rayleigh and Mie terms are the coefficient times the scale height; the
    /// ozone term is the peak coefficient times the half-width, which is the
    /// area of the triangle.
    #[must_use]
    pub fn zenith_optical_depth(&self) -> [f64; 3] {
        let mie = (self.mie_scattering + self.mie_absorption) * self.mie_height;
        let mut depth = self.rayleigh_scattering;
        for (channel, value) in depth.iter_mut().enumerate() {
            *value = *value * self.rayleigh_height
                + mie
                + self.ozone_absorption[channel] * self.ozone_width;
        }
        depth
    }

    /// Relative air mass at an apparent altitude in degrees: how many times the
    /// vertical column a slanted path is.
    ///
    /// Kasten and Young, *Applied Optics* 28, 4735 (1989). One at the zenith
    /// and 37.9 at the horizon, where the flat-earth `1 / sin(altitude)` says
    /// infinity and is wrong by a factor of anything you like. Below the
    /// horizon there is no path to space, so the argument is clamped at zero.
    #[must_use]
    pub fn air_mass(altitude: f64) -> f64 {
        let clamped = altitude.clamp(0.0, 90.0);
        1.0 / (sin(clamped) + 0.505_72 * libm::pow(clamped + 6.079_95, -1.636_4))
    }

    /// Transmittance from an observer at sea level to space, per channel,
    /// looking at an apparent altitude in degrees.
    ///
    /// The zenith optical depth scaled by air mass, which treats the ozone
    /// layer as though it sat at the ground. That overstates ozone extinction
    /// near the horizon; ozone is a fifth of the total and the horizon is
    /// where nothing is legible anyway.
    #[must_use]
    pub fn transmittance(&self, altitude: f64) -> [f64; 3] {
        let mass = Self::air_mass(altitude);
        let depth = self.zenith_optical_depth();
        let mut through = depth;
        for value in &mut through {
            *value = libm::exp(-mass * *value);
        }
        through
    }

    /// Extinction in **magnitudes** at an apparent altitude in degrees, in the
    /// green channel.
    ///
    /// What to add to a star's catalogue magnitude to get the magnitude it
    /// appears at. About 0.16 at the zenith in this clean air and six at the
    /// horizon, which is the whole reason a first-magnitude star setting looks
    /// like a fourth-magnitude one.
    #[must_use]
    pub fn extinction(&self, altitude: f64) -> f64 {
        -2.5 * libm::log10(self.transmittance(altitude)[1])
    }
}

/// The Rayleigh phase function, normalised to integrate to one over the
/// sphere.
///
/// Symmetric forward and back, which is why the sky is nearly as bright
/// opposite the sun as toward it.
#[must_use]
pub fn rayleigh_phase(cosine: f64) -> f64 {
    3.0 / (16.0 * PI) * (1.0 + cosine * cosine)
}

/// The Henyey-Greenstein phase function, normalised to integrate to one over
/// the sphere.
///
/// The standard one-parameter stand-in for Mie scattering. `asymmetry` is zero
/// for isotropic, positive for forward-scattering; take
/// [`Atmosphere::mie_asymmetry`] for air, and something lower for a thick
/// smoke.
#[must_use]
pub fn henyey_greenstein(cosine: f64, asymmetry: f64) -> f64 {
    let squared = asymmetry * asymmetry;
    let denominator = 1.0 + squared - 2.0 * asymmetry * cosine;
    (1.0 - squared) / (4.0 * PI * denominator * libm::sqrt(denominator.max(0.0)))
}
