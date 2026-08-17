//! A picture as data: a size, a format, a buffer, and the pyramid over it.

use alloc::vec::Vec;
use core::fmt;

use corvid_color::Rgba8;

use crate::{ImageError, PixelFormat};

/// A size in texels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Extent {
    /// Across.
    pub width: u32,
    /// Down.
    pub height: u32,
}

/// An [`Extent`], spelled the way a call reads.
#[must_use]
pub const fn extent(width: u32, height: u32) -> Extent {
    Extent { width, height }
}

impl Extent {
    /// A size from its two halves.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// How many texels, as a `u64` because a 131072-square plate has more than
    /// four billion of them and a `u32` would wrap silently.
    #[must_use]
    pub const fn texels(self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Whether either side is zero.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// The longer side.
    #[must_use]
    pub const fn longest(self) -> u32 {
        if self.width > self.height {
            self.width
        } else {
            self.height
        }
    }

    /// This size at mip `level`: each side halved `level` times, and never
    /// below one.
    ///
    /// Rounding *down* with a floor of one is the rule every graphics API
    /// uses, so a mip chain computed here indexes the same levels a device
    /// allocated.
    ///
    /// ```
    /// use corvid_image::extent;
    ///
    /// assert_eq!(extent(1024, 256).mip(2), extent(256, 64));
    /// // The narrow side bottoms out and the wide one keeps halving.
    /// assert_eq!(extent(1024, 256).mip(9), extent(2, 1));
    /// ```
    #[must_use]
    pub const fn mip(self, level: u32) -> Self {
        const fn half(side: u32, level: u32) -> u32 {
            if level >= u32::BITS {
                return 1;
            }
            let shifted = side >> level;
            if shifted == 0 { 1 } else { shifted }
        }
        Self::new(half(self.width, level), half(self.height, level))
    }

    /// How many mip levels this size has, counting level zero.
    ///
    /// One for a single texel, and one more for every doubling of the longer
    /// side. Zero for an empty size, which has no levels because it has no
    /// texels.
    ///
    /// ```
    /// use corvid_image::extent;
    ///
    /// assert_eq!(extent(1, 1).mip_levels(), 1);
    /// assert_eq!(extent(1024, 256).mip_levels(), 11);
    /// ```
    #[must_use]
    pub const fn mip_levels(self) -> u32 {
        if self.is_empty() {
            0
        } else {
            self.longest().ilog2() + 1
        }
    }
}

impl fmt::Display for Extent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// A decoded picture: a size, a format, and one buffer of tightly packed rows.
///
/// One level only. The pyramid over an image is described by
/// [`Extent::mip_levels`] and built on the device, because a box filter over a
/// 131072-square plate is the sort of thing a GPU finishes while a CPU is still
/// deciding which cache line to fault.
///
/// ```
/// use corvid_image::{Image, PixelFormat, extent};
///
/// let grey = Image::new(extent(2, 2), PixelFormat::R8, vec![0, 64, 128, 255])?;
/// assert_eq!(grey.texel(1, 0), Some(&[64][..]));
/// assert_eq!(grey.extent().mip_levels(), 2);
/// # Ok::<(), corvid_image::ImageError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Image {
    extent: Extent,
    format: PixelFormat,
    texels: Vec<u8>,
}

impl Image {
    /// A picture from its three parts.
    ///
    /// # Errors
    ///
    /// [`ImageError::Empty`] if either side is zero, and
    /// [`ImageError::Length`] if `texels` is not exactly
    /// `width * height * bytes_per_texel` long. Rows are tightly packed: there
    /// is no stride here, because a stride is a device's business and this is
    /// the side of the fence with no device on it.
    pub fn new(extent: Extent, format: PixelFormat, texels: Vec<u8>) -> Result<Self, ImageError> {
        if extent.is_empty() {
            return Err(ImageError::Empty(extent));
        }
        let wanted = extent.texels() * u64::from(format.bytes_per_texel());
        let given = texels.len() as u64;
        if wanted != given {
            return Err(ImageError::Length {
                extent,
                format,
                wanted,
                given,
            });
        }
        Ok(Self {
            extent,
            format,
            texels,
        })
    }

    /// How big it is.
    #[must_use]
    pub const fn extent(&self) -> Extent {
        self.extent
    }

    /// What a texel is made of.
    #[must_use]
    pub const fn format(&self) -> PixelFormat {
        self.format
    }

    /// The whole buffer, tightly packed, row-major, top row first.
    #[must_use]
    pub fn texels(&self) -> &[u8] {
        &self.texels
    }

    /// The buffer, moved out, for a caller that is about to upload it.
    #[must_use]
    pub fn into_texels(self) -> Vec<u8> {
        self.texels
    }

    /// The channels of one texel, or `None` outside the picture.
    #[must_use]
    pub fn texel(&self, x: u32, y: u32) -> Option<&[u8]> {
        if x >= self.extent.width || y >= self.extent.height {
            return None;
        }
        let stride = self.format.bytes_per_texel() as usize;
        let index = (y as usize * self.extent.width as usize + x as usize) * stride;
        self.texels.get(index..index + stride)
    }

    /// One texel as a colour.
    ///
    /// `None` outside the picture, and `None` for a
    /// [`ColorSpace::Linear`](crate::ColorSpace::Linear) image: an [`Rgba8`] is
    /// defined as sRGB storage, so answering one for linear bytes would be a
    /// lie that no later code could catch. A missing green and blue repeat the
    /// red, so a single-channel picture reads as grey rather than as red, and a
    /// missing alpha is opaque.
    ///
    /// ```
    /// use corvid_image::{Image, PixelFormat, extent};
    /// use corvid_color::Rgba8;
    ///
    /// let ink = Image::new(extent(1, 1), PixelFormat::SRGB8, vec![0x2b, 0x1a, 0x12])?;
    /// assert_eq!(ink.srgba8(0, 0), Some(Rgba8::rgb(0x2b, 0x1a, 0x12)));
    ///
    /// let mask = Image::new(extent(1, 1), PixelFormat::R8, vec![0x40])?;
    /// assert_eq!(mask.srgba8(0, 0), None);
    /// # Ok::<(), corvid_image::ImageError>(())
    /// ```
    #[must_use]
    pub fn srgba8(&self, x: u32, y: u32) -> Option<Rgba8> {
        if self.format.color_space != crate::ColorSpace::Srgb {
            return None;
        }
        let texel = self.texel(x, y)?;
        Some(match *texel {
            [grey] => Rgba8::rgb(grey, grey, grey),
            [grey, alpha] => Rgba8::new(grey, grey, grey, alpha),
            [red, green, blue] => Rgba8::rgb(red, green, blue),
            [red, green, blue, alpha] => Rgba8::new(red, green, blue, alpha),
            // `PixelFormat` has no fifth channel count and `texel` answers
            // exactly `bytes_per_texel` bytes, so this arm is unreachable
            // without a new `Channels` variant -- which is what it is here to
            // catch, since `unreachable!` is denied and would be worse anyway.
            _ => Rgba8::TRANSPARENT,
        })
    }
}
