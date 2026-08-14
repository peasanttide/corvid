//! The picture a platform puts in a title bar, a dock and a task switcher.

use corvid_color::Rgba8;

/// A game's icon: rows of [`Rgba8`], top row first.
///
/// Client-ring data with no platform in it, so a headless build can name one
/// and a game can build one at start-up without asking whether there is a
/// window to put it in. [`Render::icon`](crate::Render::icon) is where a
/// window asks for it.
///
/// The pixel type is [`Rgba8`] because that is the workspace's storage form for
/// a colour: sRGB-encoded, straight alpha, exactly comparable. An icon is
/// authored rather than computed, so it is the right side of the line
/// [`LinearRgba`](corvid_color::LinearRgba) is on the other side of.
///
/// ```
/// use corvid_color::Rgba8;
/// use corvid_render::Icon;
///
/// // Two by two, which is the smallest icon with a row order to get wrong.
/// let pixels = vec![Rgba8::WHITE, Rgba8::BLACK, Rgba8::BLACK, Rgba8::WHITE];
/// let icon = Icon::try_from((2, 2, pixels))?;
/// assert_eq!((icon.width(), icon.height()), (2, 2));
/// assert_eq!(icon.pixels().len(), 4);
///
/// // A row short is refused rather than padded: an icon whose pixels do not
/// // fill its rectangle is a picture nobody can draw.
/// assert!(Icon::try_from((2, 2, vec![Rgba8::WHITE])).is_err());
/// # Ok::<(), corvid_render::NotAnIcon>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Icon {
    /// How many pixels wide.
    width: u32,
    /// How many pixels tall.
    height: u32,
    /// The pixels, row by row, top row first.
    pixels: Vec<Rgba8>,
}

impl Icon {
    /// How many pixels across one row is.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// How many rows there are.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, row by row, top row first.
    #[must_use]
    pub fn pixels(&self) -> &[Rgba8] {
        &self.pixels
    }

    /// The pixels as the bytes a platform takes: four per pixel, `r`, `g`, `b`,
    /// `a`, in the same order.
    ///
    /// The same conversion as `Vec::<u8>::from(&icon)`, under the name a caller
    /// holding an icon reaches for.
    ///
    /// ```
    /// # use corvid_render::Icon;
    /// # use corvid_color::Rgba8;
    /// let icon = Icon::try_from((1, 1, vec![Rgba8::new(1, 2, 3, 4)])).unwrap();
    /// assert_eq!(icon.to_bytes(), [1, 2, 3, 4]);
    /// assert_eq!(Vec::<u8>::from(&icon), icon.to_bytes());
    /// ```
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.pixels.len().saturating_mul(4));
        for pixel in &self.pixels {
            bytes.extend_from_slice(&pixel.to_array());
        }
        bytes
    }
}

/// The pixels do not fill the rectangle they were given.
///
/// A refusal rather than a repair, for the reason every other bound in this
/// workspace is one: an icon padded to fit would be a different picture than
/// the one that was authored, on whichever platform happened to read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[error(
    "an icon {width} by {height} needs {} pixels and was given {pixels}",
    u64::from(*width) * u64::from(*height)
)]
pub struct NotAnIcon {
    /// How many pixels wide it was said to be.
    pub width: u32,
    /// How many rows it was said to have.
    pub height: u32,
    /// How many pixels arrived.
    pub pixels: usize,
}

/// The total half of the pair: every icon is bytes, so this cannot refuse.
///
/// By reference, because an icon owns its pixels and a platform that is handed
/// the bytes usually wants to keep the icon as well.
impl From<&Icon> for Vec<u8> {
    fn from(icon: &Icon) -> Self {
        icon.to_bytes()
    }
}

impl TryFrom<(u32, u32, Vec<Rgba8>)> for Icon {
    type Error = NotAnIcon;

    /// Builds an icon `width` by `height` out of `pixels`, or says why they do
    /// not fit.
    ///
    /// # Errors
    ///
    /// [`NotAnIcon`] when the pixel count is not `width x height`.
    fn try_from((width, height, pixels): (u32, u32, Vec<Rgba8>)) -> Result<Self, NotAnIcon> {
        let needed = u64::from(width) * u64::from(height);
        if needed != u64::try_from(pixels.len()).unwrap_or(u64::MAX) {
            return Err(NotAnIcon {
                width,
                height,
                pixels: pixels.len(),
            });
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
}
