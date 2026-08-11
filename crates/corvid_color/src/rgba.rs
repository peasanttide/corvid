//! The storage form: four bytes, sRGB-encoded, straight alpha.

#![allow(
    clippy::cast_possible_truncation,
    reason = "the hex constructors take a colour apart a byte at a time, and the truncation is what taking it apart means"
)]

use corvid_fixed::I16F16;

use crate::{LinearRgba, transfer};

/// A colour the way a palette writes one: four bytes, sRGB-encoded.
///
/// This is the *authoring* and *storage* form. It is what a hex code denotes,
/// what a texture holds, and what a golden records -- small, exactly comparable,
/// and hashable. It is **not** what arithmetic is done in: light adds and sRGB
/// codes do not, so averaging two of these directly gives a colour darker than
/// either. [`to_linear`](Self::to_linear) is the crossing, and
/// [`LinearRgba`] is the side of it where multiplying means something.
///
/// Alpha is *straight* rather than premultiplied, and it is not transferred --
/// it is a coverage fraction rather than a light level, so running it through
/// the transfer function would darken every soft edge in the game.
/// [`LinearRgba::premultiplied`] is there for the blend that wants the other
/// convention.
///
/// ```
/// use corvid_color::Rgba8;
///
/// // The two spellings a palette actually uses.
/// const EMBER: Rgba8 = Rgba8::opaque_hex(0xE5_78_29);
/// const GLASS: Rgba8 = Rgba8::hex(0xE5_78_29_80);
///
/// assert_eq!(EMBER, Rgba8::rgb(0xE5, 0x78, 0x29));
/// assert_eq!(GLASS, EMBER.with_alpha(0x80));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct Rgba8 {
    /// Red, sRGB-encoded.
    pub r: u8,
    /// Green, sRGB-encoded.
    pub g: u8,
    /// Blue, sRGB-encoded.
    pub b: u8,
    /// Coverage. Not sRGB-encoded, and not premultiplied.
    pub a: u8,
}

impl Rgba8 {
    /// Nothing at all: black with no coverage.
    ///
    /// Black rather than white under the alpha, because a filter that ignores
    /// the alpha and averages the colour anyway darkens towards this -- and a
    /// halo of dark is what every renderer's blending already expects.
    pub const TRANSPARENT: Self = Self::new(0, 0, 0, 0);

    /// Opaque black.
    pub const BLACK: Self = Self::rgb(0, 0, 0);

    /// Opaque white.
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    /// A colour and a coverage.
    #[must_use]
    #[inline]
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// An opaque colour.
    #[must_use]
    #[inline]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }

    /// A colour written the way a stylesheet writes one: `0xRRGGBBAA`.
    #[must_use]
    #[inline]
    pub const fn hex(rgba: u32) -> Self {
        Self::new(
            (rgba >> 24) as u8,
            (rgba >> 16) as u8,
            (rgba >> 8) as u8,
            rgba as u8,
        )
    }

    /// The same, without the alpha byte: `0xRRGGBB`, fully opaque.
    ///
    /// The one a palette reaches for, because a palette is almost never
    /// translucent and writing `FF` at the end of every line is noise that
    /// hides the one entry that is not.
    #[must_use]
    #[inline]
    pub const fn opaque_hex(rgb: u32) -> Self {
        Self::new((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255)
    }

    /// The same colour at a different coverage.
    #[must_use]
    #[inline]
    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// The four bytes, in `r`, `g`, `b`, `a` order.
    #[must_use]
    #[inline]
    pub const fn to_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// The linear colour this one denotes.
    ///
    /// A `const fn`, because [`decode`](crate::decode) is a table -- so a
    /// palette's linear form can be a `const` rather than something computed at
    /// start-up.
    #[must_use]
    #[inline]
    pub const fn to_linear(self) -> LinearRgba {
        LinearRgba::new(
            transfer::decode(self.r),
            transfer::decode(self.g),
            transfer::decode(self.b),
            // Not transferred: coverage is not a light level, so it takes the
            // plain `UNORM` conversion rather than the sRGB curve.
            I16F16::from_unorm8(self.a),
        )
    }
}

impl From<[u8; 4]> for Rgba8 {
    #[inline]
    fn from([r, g, b, a]: [u8; 4]) -> Self {
        Self::new(r, g, b, a)
    }
}

impl From<Rgba8> for [u8; 4] {
    #[inline]
    fn from(colour: Rgba8) -> Self {
        colour.to_array()
    }
}
