//! What a texel is made of, and which file formats are recognised.

use core::fmt;

/// How many eight-bit channels a texel carries.
///
/// The discriminant is the count, so `Channels::Rgb as u32` is three and the
/// stride arithmetic never needs a match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[repr(u8)]
pub enum Channels {
    /// One: a mask, a height field, a coverage map.
    R = 1,
    /// Two: a value and its coverage.
    Rg = 2,
    /// Three: a colour with no coverage, which is what a photograph is.
    Rgb = 3,
    /// Four: a colour and its coverage.
    #[default]
    Rgba = 4,
}

impl Channels {
    /// How many bytes one texel of this many channels weighs.
    #[must_use]
    pub const fn bytes(self) -> u32 {
        self as u32
    }

    /// The variant carrying `count` channels, or `None` outside 1..=4.
    #[must_use]
    pub const fn from_count(count: u32) -> Option<Self> {
        match count {
            1 => Some(Self::R),
            2 => Some(Self::Rg),
            3 => Some(Self::Rgb),
            4 => Some(Self::Rgba),
            _ => None,
        }
    }
}

/// Whether the stored bytes are sRGB codes or linear values.
///
/// This is the one thing about a picture that a sampler has to be told and
/// cannot infer. It is not a property of the numbers -- the same byte is a
/// legal value of either -- so getting it wrong is not an error anywhere, it is
/// a picture that comes out the wrong brightness. `corvid_color::decode` is the
/// crossing when it has to happen on a CPU; a device does it in the sampler.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub enum ColorSpace {
    /// sRGB codes, which is what every scanned plate and every hand-authored
    /// texture in an archive holds.
    #[default]
    Srgb,
    /// Linear values, which is what a normal map, a mask and a height field
    /// hold. Running one of those through a transfer function is the classic
    /// way to get a surface that lights wrong and looks nearly right.
    Linear,
}

/// A texel: a channel count and the space its bytes are in.
///
/// Eight bits per channel and no other depth. That is the whole of what the
/// archive holds and the whole of what the tile cache stores, and widening it
/// later is a new variant rather than a new type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct PixelFormat {
    /// How many channels.
    pub channels: Channels,
    /// What the bytes mean.
    pub color_space: ColorSpace,
}

impl PixelFormat {
    /// Four sRGB channels: a hand-coloured plate with its coverage.
    pub const SRGBA8: Self = Self::new(Channels::Rgba, ColorSpace::Srgb);
    /// Three sRGB channels: a photograph or a scan with no coverage.
    pub const SRGB8: Self = Self::new(Channels::Rgb, ColorSpace::Srgb);
    /// Four linear channels.
    pub const RGBA8: Self = Self::new(Channels::Rgba, ColorSpace::Linear);
    /// One linear channel: a mask, a height field, a coverage map.
    pub const R8: Self = Self::new(Channels::R, ColorSpace::Linear);

    /// A format from its two halves.
    #[must_use]
    pub const fn new(channels: Channels, color_space: ColorSpace) -> Self {
        Self {
            channels,
            color_space,
        }
    }

    /// How many bytes one texel weighs, which is the channel count.
    #[must_use]
    pub const fn bytes_per_texel(self) -> u32 {
        self.channels.bytes()
    }
}

impl fmt::Display for PixelFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let channels = match self.channels {
            Channels::R => "r8",
            Channels::Rg => "rg8",
            Channels::Rgb => "rgb8",
            Channels::Rgba => "rgba8",
        };
        let space = match self.color_space {
            ColorSpace::Srgb => "srgb",
            ColorSpace::Linear => "linear",
        };
        write!(f, "{channels} {space}")
    }
}

/// A container format, identified by the bytes it starts with.
///
/// Recognising a format is not the same as being able to decode it, and this
/// enum names one more than any build of this crate can read: see
/// [`Jpeg2000`](Self::Jpeg2000).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Codec {
    /// PNG, read by the `png` feature.
    Png,
    /// Baseline and progressive JPEG, read by the `jpeg` feature.
    Jpeg,
    /// JPEG 2000, in either the raw codestream or the JP2 box wrapping.
    ///
    /// Recognised and never decoded. [`Codec::sniff`] answers this so that a
    /// caller can say "this plate needs converting" instead of "these bytes are
    /// not an image", which is a materially different thing to put in front of
    /// somebody staring at a map that will not open.
    Jpeg2000,
}

impl Codec {
    /// The format `bytes` begins with, or `None` for a signature this crate
    /// does not know.
    ///
    /// ```
    /// use corvid_image::Codec;
    ///
    /// assert_eq!(Codec::sniff(b"\x89PNG\r\n\x1a\n....."), Some(Codec::Png));
    /// assert_eq!(Codec::sniff(b"\xff\xd8\xff\xe0"), Some(Codec::Jpeg));
    /// assert_eq!(Codec::sniff(b"\xff\x4f\xff\x51"), Some(Codec::Jpeg2000));
    /// assert_eq!(Codec::sniff(b"GIF89a"), None);
    /// ```
    #[must_use]
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
        // The start of a raw JPEG 2000 codestream: SOC then SIZ.
        const J2K: &[u8] = b"\xff\x4f\xff\x51";
        // The signature box of the JP2 container, length and all.
        const JP2: &[u8] = b"\x00\x00\x00\x0cjP  \r\n\x87\n";

        if bytes.starts_with(PNG) {
            Some(Self::Png)
        } else if bytes.starts_with(J2K) || bytes.starts_with(JP2) {
            Some(Self::Jpeg2000)
        } else if bytes.starts_with(b"\xff\xd8\xff") {
            Some(Self::Jpeg)
        } else {
            None
        }
    }

    /// Whether this build carries a decoder for this format.
    ///
    /// ```
    /// use corvid_image::Codec;
    ///
    /// // No pure-Rust decoder exists to put behind a feature, so this is false
    /// // in every build there will ever be.
    /// assert!(!Codec::Jpeg2000.is_decodable());
    /// ```
    #[must_use]
    pub const fn is_decodable(self) -> bool {
        match self {
            Self::Png => cfg!(feature = "png"),
            Self::Jpeg => cfg!(feature = "jpeg"),
            Self::Jpeg2000 => false,
        }
    }
}

impl fmt::Display for Codec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Jpeg2000 => "JPEG 2000",
        })
    }
}
