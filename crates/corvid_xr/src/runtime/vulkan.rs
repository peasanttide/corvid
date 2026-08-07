//! The seam, and the whole of it.
//!
//! `OpenXR` hands out Vulkan handles and `wgpu-hal` takes them. There is no safe
//! API on either side of that: the loader is a shared library resolved by name,
//! the handles are opaque pointers with no lifetime attached, and the image a
//! swapchain owns has to be described to `wgpu` by a caller who promises the
//! description matches. Every one of those promises is made here and nowhere
//! else.
//!
//! Four crossings, and each is one function:
//!
//! | | |
//! |---|---|
//! | [`entry`] | resolve the loader by name |
//! | [`handles`] | the instance, physical device, device and queue `wgpu` chose |
//! | [`graphics_device`] | the physical device the runtime insists on |
//! | [`session`] | hand `OpenXR` the four of them |
//! | [`texture`] | wrap one swapchain image as a `wgpu::Texture` |
//!
//! Everything above this file — poses, spaces, the anchor arithmetic, the
//! scripted headset — is ordinary safe Rust, and `tests/unsafe.rs` fails if the
//! word appears in any of it.

#![allow(
    unsafe_code,
    reason = "OpenXR hands out Vulkan handles and wgpu-hal takes them; there is no safe API on either side of that seam, and this file is the entire seam"
)]

use core::ffi::c_void;

use ash::vk::Handle as _;

use super::Unavailable;

/// The four things `OpenXR` needs to be told about the device `wgpu` created.
///
/// Raw handles, so this is deliberately not stored anywhere: it is built from a
/// borrowed [`wgpu::Device`], handed straight to [`session`], and dropped. That
/// is what keeps the "the device outlives the session" promise a local
/// argument rather than a lifetime nobody can check.
#[derive(Clone, Copy, Debug)]
pub(super) struct Handles {
    /// The `VkInstance` `wgpu` created.
    pub(super) instance: *const c_void,
    /// The `VkPhysicalDevice` it chose.
    pub(super) physical_device: *const c_void,
    /// The `VkDevice` it opened.
    pub(super) device: *const c_void,
    /// Which queue family that device's queue came from.
    pub(super) queue_family_index: u32,
    /// Which queue within it.
    pub(super) queue_index: u32,
}

/// Resolves the `OpenXR` loader by the platform's name for it.
///
/// # Errors
///
/// [`Unavailable::NoRuntime`] when there is no loader to find, which is the
/// ordinary answer on a machine with no headset software installed.
pub(super) fn entry() -> Result<openxr::Entry, Unavailable> {
    // SAFETY: `Entry::load` requires that whatever shared library answers to
    // `openxr_loader` conforms to the OpenXR specification. Nothing this
    // process does can establish that — the loader is chosen by the operating
    // system's search path — so the promise is the same one every OpenXR
    // application makes: an OpenXR loader on the search path is an OpenXR
    // loader. A machine with none returns `Err` here rather than misbehaving,
    // which is the case this crate is actually built to survive.
    unsafe { openxr::Entry::load() }.map_err(|_| Unavailable::NoRuntime)
}

/// The handles `wgpu` chose, read back out of its Vulkan backend.
///
/// # Errors
///
/// [`Unavailable::DeviceMismatch`] when the device is not a Vulkan one — a
/// Metal, DX12 or `WebGPU` device has no handles `OpenXR` could be given.
pub(super) fn handles(device: &wgpu::Device) -> Result<Handles, Unavailable> {
    // SAFETY: `Device::as_hal` requires that the backend parameter matches the
    // device's own, and it answers `None` rather than misinterpreting one when
    // it does not — that check is inside `wgpu` and is why this call can be
    // made without knowing the backend first. The handles are read out and
    // copied while the borrow is live; none of them outlives this function,
    // and `Handles` is passed by value to `session` before `device` can be
    // dropped, because `session` is called with the same borrow still held.
    let handles = unsafe {
        device
            .as_hal::<wgpu_hal::api::Vulkan>()
            .map(|vulkan| Handles {
                instance: vulkan.shared_instance().raw_instance().handle().as_raw()
                    as *const c_void,
                physical_device: vulkan.raw_physical_device().as_raw() as *const c_void,
                device: vulkan.raw_device().handle().as_raw() as *const c_void,
                queue_family_index: vulkan.queue_family_index(),
                queue_index: vulkan.queue_index(),
            })
    };
    handles.ok_or(Unavailable::DeviceMismatch)
}

/// The physical device the runtime insists the session be created on.
///
/// # Errors
///
/// [`Unavailable::Runtime`] with the runtime's own code, or
/// [`Unavailable::NoHeadset`] when it has no device to name.
pub(super) fn graphics_device(
    instance: &openxr::Instance,
    system: openxr::SystemId,
    vulkan_instance: *const c_void,
) -> Result<*const c_void, Unavailable> {
    // SAFETY: the pointer must be a live `VkInstance`. It came from `handles`
    // above, which read it from a `wgpu::Device` the caller still holds, so
    // the instance behind it is alive for the whole of this call — a `wgpu`
    // device keeps its instance alive, and the borrow is what says the device
    // has not been dropped. The runtime only reads the handle; it does not
    // take ownership of it.
    unsafe { instance.vulkan_graphics_device(system, vulkan_instance) }.map_err(Unavailable::from)
}

/// Creates the session, on the device `wgpu` opened.
///
/// # Errors
///
/// [`Unavailable::Runtime`] with the runtime's own code.
pub(super) fn session(
    instance: &openxr::Instance,
    system: openxr::SystemId,
    handles: Handles,
) -> Result<
    (
        openxr::Session<openxr::Vulkan>,
        openxr::FrameWaiter,
        openxr::FrameStream<openxr::Vulkan>,
    ),
    Unavailable,
> {
    let info = openxr::vulkan::SessionCreateInfo {
        instance: handles.instance,
        physical_device: handles.physical_device,
        device: handles.device,
        queue_family_index: handles.queue_family_index,
        queue_index: handles.queue_index,
    };
    // SAFETY: `create_session` requires that the four handles name a live
    // Vulkan device that outlives the session, that the queue be the one the
    // device was created with, and that the physical device be the one the
    // runtime named. `handles` was read from the borrowed `wgpu::Device` the
    // caller holds for the whole session — `OpenXr::open` takes it by
    // reference and the session it returns is owned by the same `OpenXr` the
    // caller keeps the device beside — the queue indices came from `wgpu`
    // itself rather than being guessed, and `graphics_device` above is the
    // check that the physical device agrees. `OpenXr::open` refuses with
    // `DeviceMismatch` when it does not.
    unsafe { instance.create_session::<openxr::Vulkan>(system, &info) }.map_err(Unavailable::from)
}

/// Wraps one swapchain image as a `wgpu::Texture`.
///
/// The descriptor must be the one the swapchain was created with; the caller
/// is [`super::Swapchain::open`], which builds both from the same values.
///
/// # Errors
///
/// [`Unavailable::DeviceMismatch`] when the device is not a Vulkan one.
pub(super) fn texture(
    device: &wgpu::Device,
    image: u64,
    format: wgpu::TextureFormat,
    size: wgpu::Extent3d,
    label: &str,
) -> Result<wgpu::Texture, Unavailable> {
    let hal = wgpu_hal::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUses::COLOR_TARGET | wgpu::TextureUses::COPY_DST,
        memory_flags: wgpu_hal::MemoryFlags::empty(),
        view_formats: Vec::new(),
    };
    let descriptor = wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    };

    // SAFETY: `as_hal` requires the backend parameter to match the device's
    // own, and it answers `None` rather than misinterpreting one when it does
    // not — that check is inside `wgpu`, which is why this call can be made
    // without knowing the backend first. The guard it returns borrows the
    // device, which the caller holds for the whole call.
    let vulkan =
        unsafe { device.as_hal::<wgpu_hal::api::Vulkan>() }.ok_or(Unavailable::DeviceMismatch)?;

    // SAFETY: `texture_from_raw` requires that `image` be a live `VkImage` on
    // this device, that `hal` describe it exactly, and that the caller say who
    // frees it. All three hold. The image came from
    // `xrEnumerateSwapchainImages` on a swapchain this process created on this
    // device moments ago, and `super::Swapchain` owns that swapchain for as
    // long as it owns the textures — its fields are dropped in declaration
    // order, so the images outlive the textures that wrap them. `hal`'s
    // extent, format, sample count and mip count are the values the swapchain
    // was created with rather than a second description of them; `super::
    // Swapchain::open` builds both from the same `size` and constant format.
    // And `TextureMemory::External` with no drop callback is the statement
    // that OpenXR frees the image when the swapchain is destroyed and `wgpu`
    // must not — which is where the double free this API exists to allow would
    // otherwise come from.
    let wrapped = unsafe {
        vulkan.texture_from_raw(
            ash::vk::Image::from_raw(image),
            &hal,
            None,
            wgpu_hal::vulkan::TextureMemory::External,
        )
    };
    drop(vulkan);

    // SAFETY: `create_texture_from_hal` requires the HAL texture to have been
    // made by this device and the descriptor to agree with the one it was made
    // from. It was made immediately above, by this device, from `hal`, and
    // `descriptor` restates the same extent, format, dimension and counts in
    // `wgpu`'s vocabulary. The initial state is `UNINITIALIZED` because an
    // image just acquired from a compositor holds nothing this frame means to
    // read.
    Ok(unsafe {
        device.create_texture_from_hal::<wgpu_hal::api::Vulkan>(
            wrapped,
            &descriptor,
            wgpu::TextureUses::UNINITIALIZED,
        )
    })
}
