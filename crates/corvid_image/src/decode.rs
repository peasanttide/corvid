//! Turning a file into an [`Image`], for the formats a build was asked for.

use crate::{Codec, DecodeError, Image};

/// Decode whatever [`Codec::sniff`] recognises the bytes as.
///
/// This function exists in every build. What changes with the `png` and `jpeg`
/// features is which formats it can carry out rather than which it can name, so
/// a build without a codec answers [`DecodeError::NoDecoder`] naming the format
/// it found instead of pretending not to recognise it.
///
/// # Errors
///
/// [`DecodeError::Unrecognised`] for bytes starting with no signature this
/// crate knows, [`DecodeError::NoDecoder`] for a format this build cannot read,
/// and [`DecodeError::Malformed`] or [`DecodeError::Unsupported`] from the
/// decoder itself.
///
/// ```
/// use corvid_image::{Codec, DecodeError, decode};
///
/// // Recognised, and never decodable: see the crate README.
/// let jp2 = b"\xff\x4f\xff\x51 and then a codestream";
/// assert_eq!(decode(jp2), Err(DecodeError::NoDecoder(Codec::Jpeg2000)));
///
/// assert_eq!(decode(b"GIF89a"), Err(DecodeError::Unrecognised));
/// ```
pub fn decode(bytes: &[u8]) -> Result<Image, DecodeError> {
    match Codec::sniff(bytes) {
        None => Err(DecodeError::Unrecognised),
        #[cfg(feature = "png")]
        Some(Codec::Png) => decode_png(bytes),
        #[cfg(feature = "jpeg")]
        Some(Codec::Jpeg) => decode_jpeg(bytes),
        Some(codec) => Err(DecodeError::NoDecoder(codec)),
    }
}

#[cfg(feature = "png")]
mod png_codec {
    use alloc::format;
    use alloc::vec;

    use crate::{Channels, Codec, ColorSpace, DecodeError, Image, PixelFormat, extent};

    /// Decode a PNG.
    ///
    /// Sixteen-bit samples are narrowed to eight and a palette is expanded to
    /// its colours, because the tile cache stores eight bits per channel and
    /// nothing downstream has a format for anything else. The result is
    /// [`ColorSpace::Srgb`]: PNG's own default rendering intent is sRGB, and a
    /// scan of a printed plate is sRGB whatever its chunks claim.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Malformed`] from the decoder, and
    /// [`DecodeError::Unsupported`] for a colour type with no
    /// [`PixelFormat`].
    pub fn decode_png(bytes: &[u8]) -> Result<Image, DecodeError> {
        let malformed = |reason: alloc::string::String| DecodeError::Malformed {
            codec: Codec::Png,
            reason,
        };
        // `png` reads through `BufRead + Seek`, which a slice is not, so the
        // slice is wrapped rather than copied.
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder
            .read_info()
            .map_err(|err| malformed(format!("{err}")))?;
        let mut buffer = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
        let frame = reader
            .next_frame(&mut buffer)
            .map_err(|err| malformed(format!("{err}")))?;
        let channels = match frame.color_type {
            png::ColorType::Grayscale => Channels::R,
            png::ColorType::GrayscaleAlpha => Channels::Rg,
            png::ColorType::Rgb => Channels::Rgb,
            png::ColorType::Rgba => Channels::Rgba,
            png::ColorType::Indexed => {
                return Err(DecodeError::Unsupported {
                    codec: Codec::Png,
                    reason: "a palette survived the expansion transformation",
                });
            }
        };
        buffer.truncate(frame.buffer_size());
        Ok(Image::new(
            extent(frame.width, frame.height),
            PixelFormat::new(channels, ColorSpace::Srgb),
            buffer,
        )?)
    }
}

#[cfg(feature = "png")]
pub use png_codec::decode_png;

#[cfg(feature = "jpeg")]
mod jpeg_codec {
    use alloc::format;

    use crate::{Channels, Codec, ColorSpace, DecodeError, Image, PixelFormat, extent};

    /// Decode a baseline or progressive JPEG.
    ///
    /// Answers whatever the file's own colour model narrows to: one channel for
    /// a greyscale scan, three for anything else. There is no alpha in a JPEG,
    /// which is why the archive's plates that need one are PNGs.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Malformed`] from the decoder, and
    /// [`DecodeError::Unsupported`] for an output colour model with no
    /// [`PixelFormat`] -- CMYK, which a scanner occasionally produces.
    pub fn decode_jpeg(bytes: &[u8]) -> Result<Image, DecodeError> {
        // A `ZCursor` rather than a slice: the decoder reads through a trait
        // that is implemented for it and for `BufRead + Seek`, and this is the
        // half of that pair that exists without an operating system.
        let mut decoder =
            zune_jpeg::JpegDecoder::new(zune_jpeg::zune_core::bytestream::ZCursor::new(bytes));
        let pixels = decoder.decode().map_err(|err| DecodeError::Malformed {
            codec: Codec::Jpeg,
            reason: format!("{err:?}"),
        })?;
        let info = decoder.info().ok_or(DecodeError::Malformed {
            codec: Codec::Jpeg,
            reason: format!("the decoder produced {} bytes and no header", pixels.len()),
        })?;
        let components = decoder
            .output_colorspace()
            .map_or(0, |space| space.num_components());
        let channels = u32::try_from(components)
            .ok()
            .and_then(Channels::from_count)
            .ok_or(DecodeError::Unsupported {
                codec: Codec::Jpeg,
                reason: "the output colour model is not one to four eight-bit channels",
            })?;
        Ok(Image::new(
            extent(u32::from(info.width), u32::from(info.height)),
            PixelFormat::new(channels, ColorSpace::Srgb),
            pixels,
        )?)
    }
}

#[cfg(feature = "jpeg")]
pub use jpeg_codec::decode_jpeg;
