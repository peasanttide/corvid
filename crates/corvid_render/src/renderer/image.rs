//! One frame off the device, and the file it is written as.

use crate::renderer::{Error, Extent};

/// One frame, read back off the device.
///
/// Four bytes per pixel, row by row from the top, in the order red, green,
/// blue, alpha. [`to_png`](Self::to_png) is what a capture writes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Image {
    /// How big it is.
    pub size: Extent,
    /// `size.width * size.height * 4` bytes.
    pub pixels: Vec<u8>,
}

impl Image {
    /// The same frame as the bytes of a PNG file.
    ///
    /// Lossless and eight bits a channel, so what comes back out of a decoder
    /// is what went in -- which is what lets a capture be compared at all.
    ///
    /// # Errors
    ///
    /// [`Error::NotRead`], reused rather than given a variant of its own,
    /// because everything the encoder can refuse here is this image being
    /// malformed: a `pixels` that is not four bytes per pixel of `size`. A
    /// [`read_back`](crate::Renderer::read_back) result never is.
    pub fn to_png(&self) -> Result<Vec<u8>, Error> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, self.size.width, self.size.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|why| Error::NotRead(why.to_string()))?;
        writer
            .write_image_data(&self.pixels)
            .map_err(|why| Error::NotRead(why.to_string()))?;
        writer
            .finish()
            .map_err(|why| Error::NotRead(why.to_string()))?;
        Ok(bytes)
    }
}
