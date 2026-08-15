//! Choosing a device, and the variable that pins which one.
//!
//! The seam is which adapter a run on this machine opens: everything here
//! answers that question or acts on the answer, and `mod.rs` above is what
//! holds the device once it is open.

use crate::renderer::{Error, Extent, Unacquired};

/// Whether the adapter this machine would draw with is a software rasteriser.
///
/// This is what makes the exact-match arm of a PNG comparison expressible.
/// Rasterisation differs between drivers, so a captured frame is compared with
/// a tolerance in general -- but two runs on the *same* software rasteriser
/// produce the same bytes, so a build machine with `lavapipe` can demand
/// equality and catch a change a tolerance would absorb.
///
/// It requests an adapter with the options [`Renderer::offscreen`](crate::Renderer::offscreen) uses and
/// drops it again, so it names the adapter a run on this machine would open
/// rather than whichever one happens to be first. A machine with no adapter at
/// all answers `false`, because there is nothing there to pin an exact
/// comparison to.
///
/// With [`SOFTWARE`] set it names the fallback adapter, which is the one every
/// path in this module opens under that variable -- so the answer stays "the one
/// a run on this machine would open" rather than becoming a claim about a
/// different device.
#[must_use]
pub fn adapter_is_software() -> bool {
    let instance = instance();
    request(&instance, None)
        .is_ok_and(|adapter| adapter.get_info().device_type == wgpu::DeviceType::Cpu)
}

/// The variable that makes every adapter request in this module ask for the
/// fallback.
///
/// A frame golden's exact-match arm is pinned to a software rasteriser, and a
/// developer machine usually has a GPU that `wgpu` will hand over first --
/// `WGPU_ADAPTER_NAME` does not help, because that is read by
/// `wgpu::util::initialize_adapter_from_env` and nothing here calls it. Setting
/// this to anything non-empty passes `force_fallback_adapter` instead, which is
/// `lavapipe` on a machine with Mesa and WARP on Windows.
///
/// **Two software rasterisers are not one adapter.** Blessing a golden under
/// this variable pins it to whichever fallback *this* machine has, and a build
/// machine with a different one is in the same position it would be with a GPU.
/// What the variable buys is that the pinning is a decision somebody made
/// rather than an accident of which adapter was first.
pub const SOFTWARE: &str = "CORVID_SOFTWARE_ADAPTER";

/// Whether [`SOFTWARE`] asks for the fallback adapter.
pub(super) fn force_fallback() -> bool {
    std::env::var_os(SOFTWARE).is_some_and(|value| !value.is_empty())
}

/// The instance every path here opens.
pub(super) fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
}

/// Asks for the adapter a run on this machine would use.
pub(super) fn request(
    instance: &wgpu::Instance,
    compatible: Option<&wgpu::Surface<'static>>,
) -> Result<wgpu::Adapter, Error> {
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: force_fallback(),
        compatible_surface: compatible,
        apply_limit_buckets: false,
    }))
    .map_err(Error::NoAdapter)
}

/// Opens an adapter and a device.
pub(super) fn open(
    instance: &wgpu::Instance,
    compatible: Option<&wgpu::Surface<'static>>,
) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), Error> {
    let adapter = request(instance, compatible)?;
    let info = adapter.get_info();
    tracing::info!(
        name: "corvid_render.adapter",
        adapter = %info.name,
        backend = %info.backend,
        device_type = ?info.device_type,
        "opened an adapter",
    );
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("corvid_render"),
        // Nothing here needs anything past what every target has, which is what
        // keeps this from being the reason a machine cannot run the game. A
        // game that needs more opens its own device; this is the one a runtime
        // opens for it.
        required_features: wgpu::Features::empty(),
        // The downlevel baseline for everything except how big a texture may
        // be, which is raised to whatever this adapter actually offers.
        //
        // The baseline's own answer is 2048, and a swapchain is a texture: a
        // maximised window on any display wider than that is not a recoverable
        // `Error` but a `ConfigureSurfaceError::TooLarge` on an infallible
        // `configure`, which `wgpu`'s default handler turns into a panic. The
        // same 2048 caps `Renderer::offscreen`, whose signature promises a
        // `Result` it never gets to return. `using_resolution` is `wgpu`'s own
        // constructor for this: the portable floor for every limit that
        // describes what a shader may do, and the hardware's ceiling for the
        // one that only describes how many pixels there are.
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .map_err(Error::NoDevice)?;
    Ok((adapter, device, queue))
}

/// Which of a surface's formats to configure it with.
///
/// A non-`Srgb` format is preferred, so that what a game's shader writes
/// reaches the display as the numbers it wrote: this crate has no colour
/// management, and an `Srgb` target would encode values that were never
/// decoded.
pub(super) fn choose_format(formats: &[wgpu::TextureFormat]) -> wgpu::TextureFormat {
    formats
        .iter()
        .copied()
        .find(|format| !format.is_srgb())
        .or_else(|| formats.first().copied())
        .unwrap_or(wgpu::TextureFormat::Bgra8Unorm)
}

/// Which [`Unacquired`] a `wgpu` answer means.
pub(super) const fn unacquired(answer: &wgpu::CurrentSurfaceTexture) -> Unacquired {
    match answer {
        wgpu::CurrentSurfaceTexture::Lost => Unacquired::Lost,
        wgpu::CurrentSurfaceTexture::Outdated => Unacquired::Outdated,
        _ => Unacquired::Validation,
    }
}

/// The colour texture an offscreen renderer draws into.
pub(super) fn colour_texture(
    device: &wgpu::Device,
    size: Extent,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("corvid_render.offscreen"),
        size: wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
