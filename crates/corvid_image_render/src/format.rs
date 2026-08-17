//! What a `corvid_image` pixel format is on a device.
//!
//! The seam is that a device has fewer formats than a file does. There is no
//! three-channel texture in any modern graphics API and no one- or two-channel
//! sRGB one, so this is where a picture's format becomes a texture's and where
//! the two cases that cost something -- an expansion and a refusal -- are
//! spelled out.

use corvid_image::{Channels, ColorSpace, PixelFormat};

/// The texture format a picture of this format lives in, or `None` for one no
/// eight-bit texture holds.
///
/// Three channels become four. `wgpu` has no `Rgb8` and neither does any device
/// behind it -- a three-byte texel has no aligned row -- so a
/// [`PixelFormat::SRGB8`] scan is stored with an opaque alpha, and the upload
/// pays one pass over the tile to put it there. That is the whole cost of the
/// archive being mostly three-channel scans, and it is paid once per tile
/// rather than once per frame.
///
/// A one- or two-channel *sRGB* picture is the refusal: no graphics API has a
/// transfer function on those, so there is nothing to answer. A mask is linear
/// and saying so is not a workaround.
///
/// ```
/// use corvid_image::PixelFormat;
/// use corvid_image_render::texture_format;
///
/// assert_eq!(texture_format(PixelFormat::SRGB8), Some(wgpu::TextureFormat::Rgba8UnormSrgb));
/// assert_eq!(texture_format(PixelFormat::R8), Some(wgpu::TextureFormat::R8Unorm));
/// ```
#[must_use]
pub const fn texture_format(format: PixelFormat) -> Option<wgpu::TextureFormat> {
    match (format.channels, format.color_space) {
        (Channels::R, ColorSpace::Linear) => Some(wgpu::TextureFormat::R8Unorm),
        (Channels::Rg, ColorSpace::Linear) => Some(wgpu::TextureFormat::Rg8Unorm),
        (Channels::Rgb | Channels::Rgba, ColorSpace::Linear) => {
            Some(wgpu::TextureFormat::Rgba8Unorm)
        }
        (Channels::Rgb | Channels::Rgba, ColorSpace::Srgb) => {
            Some(wgpu::TextureFormat::Rgba8UnormSrgb)
        }
        (Channels::R | Channels::Rg, ColorSpace::Srgb) => None,
    }
}

/// How many bytes one texel of this format weighs *on the device*, which is not
/// always what it weighs in a file.
///
/// Three-channel pictures cost four here, and that is the number every budget
/// in this crate is computed from: a cache sized from the file's three bytes
/// would be a third over its budget on the first frame.
///
/// ```
/// use corvid_image::PixelFormat;
/// use corvid_image_render::device_bytes_per_texel;
///
/// assert_eq!(PixelFormat::SRGB8.bytes_per_texel(), 3);
/// assert_eq!(device_bytes_per_texel(PixelFormat::SRGB8), 4);
/// ```
#[must_use]
pub const fn device_bytes_per_texel(format: PixelFormat) -> u32 {
    match format.channels {
        Channels::R => 1,
        Channels::Rg => 2,
        Channels::Rgb | Channels::Rgba => 4,
    }
}

/// Copies `rows` of `texels` into `scratch` at `stride` bytes a row, widening a
/// three-channel texel to four on the way.
///
/// `stride` is the padded row length the queue is told about, which is why the
/// gap between the end of a row and the start of the next is left alone rather
/// than written: it is never read.
pub(crate) fn stage(
    scratch: &mut [u8],
    texels: &[u8],
    format: PixelFormat,
    width: usize,
    rows: usize,
    stride: usize,
) {
    let source = format.bytes_per_texel() as usize;
    let target = device_bytes_per_texel(format) as usize;
    for row in 0..rows {
        let from = texels.get(row * width * source..(row + 1) * width * source);
        let into = scratch.get_mut(row * stride..row * stride + width * target);
        let (Some(from), Some(into)) = (from, into) else {
            continue;
        };
        if source == target {
            into.copy_from_slice(from);
            continue;
        }
        // The one widening: three channels in, four out, alpha opaque. The
        // chunks are exact because both slices were cut to `width` texels.
        for (texel, out) in from.chunks_exact(source).zip(into.chunks_exact_mut(target)) {
            for (channel, byte) in texel.iter().zip(out.iter_mut()) {
                *byte = *channel;
            }
            if let Some(alpha) = out.get_mut(source..target) {
                alpha.fill(u8::MAX);
            }
        }
    }
}
