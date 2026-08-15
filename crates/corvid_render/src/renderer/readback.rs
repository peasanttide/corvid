//! Reading a finished frame back off the device.
//!
//! The seam against `mod.rs` is which way the pixels travel. Everything there
//! writes into a target; this is the one path that copies out of one, and it
//! is the only place in the crate that waits on a device.

use crate::renderer::{Canvas, Error, Image, PATIENCE, Renderer};

impl Renderer {
    /// Reads the last drawn frame back off the device.
    ///
    /// This is the whole capture seam. There is no draw list to serialize any
    /// more, so what a headless run can be compared on is the pixels a real
    /// adapter produced -- and the crate documentation is exact about how much
    /// weaker a golden that makes.
    ///
    /// # Errors
    ///
    /// [`Error::NotOffscreen`] on a renderer that draws into a window, and
    /// [`Error::NotRead`] if the device will not hand the bytes over.
    pub fn read_back(&self) -> Result<Image, Error> {
        let Canvas::Offscreen { texture, .. } = &self.canvas else {
            return Err(Error::NotOffscreen);
        };

        // A copy out of a texture writes rows padded to 256 bytes, so the
        // buffer is wider than the image and the padding is dropped below.
        let unpadded = self.size.width * 4;
        let padded = unpadded.div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("corvid_render.readback"),
            size: u64::from(padded) * u64::from(self.size.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("corvid_render.readback"),
            });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(self.size.height),
                },
            },
            wgpu::Extent3d {
                width: self.size.width,
                height: self.size.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(std::iter::once(encoder.finish()));

        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            // The receiver is on the stack below and outlives this, so the
            // only way the send fails is a device that vanished, in which case
            // the `recv` below reports the disconnect instead.
            drop(sender.send(result));
        });
        drop(self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(PATIENCE),
        }));
        match receiver.recv_timeout(PATIENCE) {
            Ok(Ok(())) => {}
            Ok(Err(why)) => return Err(Error::NotRead(why.to_string())),
            Err(why) => return Err(Error::NotRead(format!("{why} after {PATIENCE:?}"))),
        }

        let mapped = slice
            .get_mapped_range()
            .map_err(|why| Error::NotRead(why.to_string()))?;
        let mut pixels = Vec::with_capacity((unpadded * self.size.height) as usize);
        for row in 0..self.size.height {
            let start = (row * padded) as usize;
            pixels.extend_from_slice(&mapped[start..start + unpadded as usize]);
        }
        drop(mapped);
        buffer.unmap();

        Ok(Image {
            size: self.size,
            pixels,
        })
    }
}
