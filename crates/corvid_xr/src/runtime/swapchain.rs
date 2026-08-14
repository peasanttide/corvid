//! The per-eye render target, and how big it is.
//!
//! The seam against `mod.rs` is the frame: a session decides *when* to draw
//! and this is *what into*. It is the only file here that hands `wgpu` a
//! texture, which is why the multiview and two-pass paths are both stated in
//! one place.

use crate::runtime::{PATIENCE, Unavailable, vulkan};

/// The per-eye render target.
///
/// One array texture with two layers when the device offers multiview, and two
/// single-layer textures when it does not. CI's software adapter does not, so
/// the two-pass path is the one that is tested and the multiview path is the
/// one only a headset runs.
pub struct Swapchain {
    /// The runtime's swapchain. Owns the images the textures below wrap, and is
    /// dropped after them.
    inner: openxr::Swapchain<openxr::Vulkan>,
    /// One per swapchain image.
    textures: Vec<wgpu::Texture>,
    /// How big each eye's image is.
    extent: Extent,
    /// Whether the two eyes are two layers of one texture.
    multiview: bool,
}

impl Swapchain {
    /// The format asked for, and the only one this crate reads.
    ///
    /// `VK_FORMAT_R8G8B8A8_SRGB`, which every runtime offers and which `wgpu`
    /// calls `Rgba8UnormSrgb`.
    const FORMAT: u32 = 43;

    /// Creates the swapchain and wraps its images.
    pub(super) fn open(
        session: &openxr::Session<openxr::Vulkan>,
        device: &wgpu::Device,
        extent: Extent,
    ) -> Result<Self, Unavailable> {
        let offered = session
            .enumerate_swapchain_formats()
            .map_err(Unavailable::from)?;
        if !offered.contains(&Self::FORMAT) {
            return Err(Unavailable::NoFormat);
        }
        let multiview = device.features().contains(wgpu::Features::MULTIVIEW);
        let layers = if multiview { 2 } else { 1 };

        let inner = session
            .create_swapchain(&openxr::SwapchainCreateInfo {
                create_flags: openxr::SwapchainCreateFlags::EMPTY,
                usage_flags: openxr::SwapchainUsageFlags::COLOR_ATTACHMENT
                    | openxr::SwapchainUsageFlags::SAMPLED,
                format: Self::FORMAT,
                sample_count: 1,
                width: extent.width,
                height: extent.height,
                face_count: 1,
                array_size: layers,
                mip_count: 1,
            })
            .map_err(Unavailable::from)?;

        let size = wgpu::Extent3d {
            width: extent.width,
            height: extent.height,
            depth_or_array_layers: layers,
        };
        let images = inner.enumerate_images().map_err(Unavailable::from)?;
        let mut textures = Vec::with_capacity(images.len());
        for image in images {
            textures.push(vulkan::texture(
                device,
                image,
                wgpu::TextureFormat::Rgba8UnormSrgb,
                size,
                "corvid_xr swapchain",
            )?);
        }

        Ok(Self {
            inner,
            textures,
            extent,
            multiview,
        })
    }

    /// Takes this frame's image, and says which it is.
    ///
    /// Blocks until the compositor has finished with it, so a frame drawn into
    /// it is a frame the compositor has not read.
    ///
    /// # Errors
    ///
    /// [`Unavailable::Runtime`] when the runtime refused, which includes the
    /// timeout it answers when the compositor is not keeping up.
    pub fn acquire(&mut self) -> Result<u32, Unavailable> {
        let index = self.inner.acquire_image().map_err(Unavailable::from)?;
        self.inner
            .wait_image(openxr::Duration::from_nanos(PATIENCE))
            .map_err(Unavailable::from)?;
        Ok(index)
    }

    /// Hands this frame's image back to the compositor.
    ///
    /// # Errors
    ///
    /// [`Unavailable::Runtime`] when the runtime refused.
    pub fn release(&mut self) -> Result<(), Unavailable> {
        self.inner.release_image().map_err(Unavailable::from)
    }

    /// One of the swapchain's textures, by the index [`acquire`](Self::acquire)
    /// answered with.
    #[must_use]
    pub fn texture(&self, index: u32) -> Option<&wgpu::Texture> {
        self.textures.get(index as usize)
    }

    /// Whether the two eyes are two layers of one texture.
    #[must_use]
    pub const fn multiview(&self) -> bool {
        self.multiview
    }

    /// How big each eye's image is.
    #[must_use]
    pub const fn extent(&self) -> Extent {
        self.extent
    }

    /// How many images the runtime is cycling through.
    #[must_use]
    pub const fn images(&self) -> usize {
        self.textures.len()
    }
}

/// How big an image is, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Extent {
    /// Across.
    pub width: u32,
    /// Down.
    pub height: u32,
}

/// The shape rather than the handles.
///
/// Hand-written because none of the runtime's types implement `Debug`, so
/// there is nothing to derive one from.
impl core::fmt::Debug for Swapchain {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Swapchain")
            .field("extent", &self.extent)
            .field("multiview", &self.multiview)
            .field("images", &self.textures.len())
            .finish_non_exhaustive()
    }
}
