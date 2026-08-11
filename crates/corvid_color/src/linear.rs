//! The arithmetic form: four fixed-point channels, linear.

use corvid_fixed::{Factor32, I16F16};

use crate::{Rgba8, transfer};

/// A colour with the transfer function taken off: four [`I16F16`]s, linear.
///
/// This is the side of the crossing where arithmetic means something. Light
/// adds, so a sum of two of these is the colour of both lights together and a
/// midpoint is the colour half way between; the same operations on an
/// [`Rgba8`] are not wrong so much as answers to a different question. It is
/// also what a shader wants: a `wgpu` render target in a `*Srgb` format
/// converts on the way out, so what a uniform buffer should hold is this --
/// through [`to_f32_array`](Self::to_f32_array), which is the one place a
/// float appears.
///
/// # Why fixed point
///
/// Because everything else in this workspace is, and because the three things
/// that buys are each worth having for a colour:
///
/// - **It compares.** `I16F16` is `Eq` and `Ord`, so this type is too -- where
///   an `f32` colour could only ever be `PartialEq`, and a `NaN` channel would
///   compare equal to nothing at all including itself.
/// - **It hashes.** `f32` and `f64` have no [`Hash`](core::hash::Hash). A
///   fixed-point colour goes in a golden, in a UI layout digest and in a
///   capture; a floating-point one could not.
/// - **It has no `NaN`.** Every operation here saturates, so a colour arriving
///   from a readback or a file has two failure modes rather than three, and the
///   third -- a value that poisons every comparison it touches -- is not
///   expressible.
///
/// Sixteen fractional bits is 1.5e-5, which is twenty steps at the darkest
/// representable sRGB code and 583 at the brightest; sixteen integer bits give
/// headroom to 32 768 for an emissive or an HDR value that leaves the unit
/// cube. See [`decode`](crate::decode) for the resolution argument in full.
///
/// ```
/// use corvid_color::{LinearRgba, Rgba8};
///
/// // Half way between black and white in *light*, which is a good deal
/// // brighter than the sRGB code half way between them.
/// let midpoint = LinearRgba::BLACK.lerp(LinearRgba::WHITE, corvid_fixed::Factor32::from_f64(0.5));
/// assert_eq!(midpoint.to_srgb8(), Rgba8::rgb(188, 188, 188));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct LinearRgba {
    /// Red, linear.
    pub r: I16F16,
    /// Green, linear.
    pub g: I16F16,
    /// Blue, linear.
    pub b: I16F16,
    /// Coverage.
    ///
    /// Linear already, and not premultiplied. Expected in `0 ..= 1`; a value
    /// outside that is clamped where it becomes a byte rather than being
    /// rejected here, because this type is also what an intermediate in a blend
    /// is and an intermediate is allowed to be out of range.
    pub a: I16F16,
}

impl LinearRgba {
    /// Nothing at all: black with no coverage.
    pub const TRANSPARENT: Self = Self::new(I16F16::ZERO, I16F16::ZERO, I16F16::ZERO, I16F16::ZERO);

    /// Opaque black.
    pub const BLACK: Self = Self::new(I16F16::ZERO, I16F16::ZERO, I16F16::ZERO, I16F16::ONE);

    /// Opaque white.
    pub const WHITE: Self = Self::new(I16F16::ONE, I16F16::ONE, I16F16::ONE, I16F16::ONE);

    /// A colour and a coverage.
    #[must_use]
    #[inline]
    pub const fn new(r: I16F16, g: I16F16, b: I16F16, a: I16F16) -> Self {
        Self { r, g, b, a }
    }

    /// An opaque colour.
    #[must_use]
    #[inline]
    pub const fn rgb(r: I16F16, g: I16F16, b: I16F16) -> Self {
        Self::new(r, g, b, I16F16::ONE)
    }

    /// The four channels, in `r`, `g`, `b`, `a` order.
    #[must_use]
    #[inline]
    pub const fn to_array(self) -> [I16F16; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// The four channels as `f32`, which is what a uniform buffer takes.
    ///
    /// **The one place a float appears in this crate**, and it is at the same
    /// boundary `corvid_render::matrix` sits on: everything above is
    /// fixed-point and everything below is what a GPU has. Nothing downstream
    /// of this is hashed, so the rounding is free.
    #[must_use]
    #[inline]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "this is the crate's boundary with the f32 a device wants, and the narrowing is what crossing it means; nothing downstream is hashed, sent or replayed"
    )]
    pub const fn to_f32_array(self) -> [f32; 4] {
        [
            self.r.to_f64() as f32,
            self.g.to_f64() as f32,
            self.b.to_f64() as f32,
            self.a.to_f64() as f32,
        ]
    }

    /// The same colour at a different coverage.
    #[must_use]
    #[inline]
    pub const fn with_alpha(self, a: I16F16) -> Self {
        Self { a, ..self }
    }

    /// The nearest 8-bit sRGB colour.
    ///
    /// The inverse of [`Rgba8::to_linear`] for every colour that came from
    /// one -- all 2^3^2 of them, not merely most.
    ///
    /// A method rather than a `From`, where the other direction is a `From`:
    /// this one clamps. A channel outside `0 ..= 1` -- an emissive, an HDR
    /// value, or an out-of-gamut Oklab conversion, all of which [`LinearRgba`]
    /// has the range to hold -- loses the whole of its excess here, and a
    /// `.into()` on the way into a texture would be that loss written nowhere.
    #[must_use]
    #[inline]
    pub const fn to_srgb8(self) -> Rgba8 {
        Rgba8::new(
            transfer::encode(self.r),
            transfer::encode(self.g),
            transfer::encode(self.b),
            // Not transferred, for the reason `Rgba8`'s alpha is not: coverage
            // is not a light level.
            self.a.to_unorm8(),
        )
    }

    /// Mixes towards `to`, linearly, per channel.
    ///
    /// Exact at both ends, because [`I16F16::lerp`] is: at
    /// [`Factor32::ZERO`] this is `self` and at [`Factor32::ONE`] it is `to`,
    /// bit for bit, which is what every interpolation in this workspace owes.
    ///
    /// This is a mix in *light*, which is right for compositing and blending
    /// and wrong for a colour ramp somebody has to look at --
    /// [`Oklab::lerp`](crate::Oklab::lerp) is the one that stays saturated
    /// through the middle.
    #[must_use]
    #[inline]
    pub const fn lerp(self, to: Self, weight: Factor32) -> Self {
        Self::new(
            self.r.lerp(to.r, weight),
            self.g.lerp(to.g, weight),
            self.b.lerp(to.b, weight),
            self.a.lerp(to.a, weight),
        )
    }

    /// The same colour with its coverage folded into its channels.
    ///
    /// The convention a `wgpu` blend state of `One`/`OneMinusSrcAlpha` reads,
    /// as against the `SrcAlpha`/`OneMinusSrcAlpha` that straight alpha wants.
    /// Corvid has no opinion about which a game picks; this is the conversion,
    /// so that the choice does not have to be made in a shader.
    #[must_use]
    #[inline]
    pub const fn premultiplied(self) -> Self {
        Self::new(
            self.r.saturating_mul(self.a),
            self.g.saturating_mul(self.a),
            self.b.saturating_mul(self.a),
            self.a,
        )
    }
}

impl From<Rgba8> for LinearRgba {
    #[inline]
    fn from(colour: Rgba8) -> Self {
        colour.to_linear()
    }
}

impl From<LinearRgba> for [I16F16; 4] {
    #[inline]
    fn from(colour: LinearRgba) -> Self {
        colour.to_array()
    }
}

impl From<[I16F16; 4]> for LinearRgba {
    #[inline]
    fn from([r, g, b, a]: [I16F16; 4]) -> Self {
        Self::new(r, g, b, a)
    }
}
