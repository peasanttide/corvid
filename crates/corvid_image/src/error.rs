//! Everything this crate refuses to do, and the reason each refusal exists.

use alloc::string::String;

use crate::{Codec, Extent, PixelFormat};

/// A [`TileConfig`](crate::TileConfig) that cannot be honoured.
///
/// Every variant here is a shape the packed table entry cannot represent.
/// Checking them once, when the configuration is built, is what lets
/// [`TileEntry`](crate::TileEntry) pack and unpack with plain shifts and no
/// error path at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// The tile side is not a power of two, or is outside 16..=16384 texels.
    ///
    /// A power of two is not a taste: the page index of a texel is a shift, and
    /// the offset inside a tile is a mask. Neither is a division here or in the
    /// shader.
    #[error("a tile side of {0} is not a power of two in 16..=16384")]
    TileSize(u32),
    /// The largest addressable image is not a power of two, or is smaller than
    /// one tile.
    #[error("a maximum image side of {size} is not a power of two of at least {tile}")]
    MaxImageSize {
        /// The rejected side, in texels.
        size: u32,
        /// The tile side it was measured against.
        tile: u32,
    },
    /// More tiles than the slot field of an entry can name.
    ///
    /// The count itself is the limit rather than the count minus one: the
    /// all-ones slot is spent on "no tile here", which is the value every
    /// entry of a freshly built table holds.
    #[error("a budget of {tiles} tiles does not fit the {bits} bits an entry names a slot with")]
    TileCount {
        /// The rejected tile count.
        tiles: u32,
        /// How many bits the entry spends on a slot.
        bits: u32,
    },
    /// More sources than a [`SourceId`](crate::SourceId) can name.
    #[error("a budget of {0} sources does not fit the byte a source id is")]
    SourceCount(u32),
}

/// A picture that cannot be held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum ImageError {
    /// Zero width or zero height.
    ///
    /// An empty picture is not a picture with no pixels in it; it is almost
    /// always a header that was read wrong, and answering an error here is what
    /// stops that from becoming a division by zero three calls later.
    #[error("an image of {0} has no texels")]
    Empty(Extent),
    /// The buffer is not exactly `width * height * bytes_per_texel` long.
    #[error("an image of {extent} in {format} wants {wanted} bytes and was given {given}")]
    Length {
        /// The declared size.
        extent: Extent,
        /// The declared format.
        format: PixelFormat,
        /// How many bytes that pair demands.
        wanted: u64,
        /// How many bytes arrived.
        given: u64,
    },
}

/// A picture that cannot be read out of its bytes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum DecodeError {
    /// The bytes begin with no signature this crate knows.
    #[error("the bytes carry no image signature this crate recognises")]
    Unrecognised,
    /// The format was recognised and this build contains no decoder for it.
    ///
    /// This is the honest answer to a feature that is off, and it is also the
    /// permanent answer for [`Codec::Jpeg2000`]: see the crate README for why
    /// there is no decoder to switch on.
    #[error("this build has no decoder for {0}")]
    NoDecoder(Codec),
    /// The decoder ran and the bytes were wrong.
    #[error("the {codec} could not be decoded: {reason}")]
    Malformed {
        /// Which decoder said so.
        codec: Codec,
        /// What it said.
        reason: String,
    },
    /// The decoder ran and the picture is a shape this crate has no format for.
    #[error("the {codec} is a form this crate has no pixel format for: {reason}")]
    Unsupported {
        /// Which decoder said so.
        codec: Codec,
        /// What about it.
        reason: &'static str,
    },
    /// The decoded picture is not a picture.
    #[error(transparent)]
    Image(#[from] ImageError),
}

/// A source that cannot be planned for.
///
/// Every one of these is refused when the source is registered rather than
/// papered over when it is drawn, because the alternative to refusing an
/// oversized plate is silently truncating it, and a map that is quietly missing
/// its eastern third looks exactly like a map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum TileError {
    /// The image is wider or taller than the configured maximum.
    #[error("an image of {extent} is larger than the configured maximum of {max} texels a side")]
    TooLarge {
        /// The rejected size.
        extent: Extent,
        /// [`TileConfig::max_image_size`](crate::TileConfig::max_image_size).
        max: u32,
    },
    /// The image has no texels.
    #[error("an image of {extent} has no texels")]
    Empty {
        /// The rejected size.
        extent: Extent,
    },
    /// The planner already holds
    /// [`TileConfig::max_sources`](crate::TileConfig::max_sources) sources.
    #[error("the planner already holds its configured maximum of {0} sources")]
    TooManySources(u32),
}
