//! Why a headset is not there.
//!
//! The seam is that nothing here has a session: this is the answer a caller
//! gets before one exists, which is why it is the one part of the runtime a
//! machine with no headset still reaches.

/// Why a headset is not there.
///
/// A reason a person can act on rather than a number, except in the one case
/// where the runtime gave a number and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum Unavailable {
    /// No `OpenXR` runtime is installed, or the one that is does not do Vulkan.
    #[error("no OpenXR runtime with Vulkan support is installed")]
    NoRuntime,
    /// One is, and it has no headset attached.
    #[error("an OpenXR runtime is installed, with no headset")]
    NoHeadset,
    /// The runtime does not offer a format `wgpu` can use.
    #[error("the runtime offers no swapchain format wgpu can use")]
    NoFormat,
    /// The Vulkan device `OpenXR` wants is not the one `wgpu` created.
    ///
    /// Also what a device on another backend answers: a Metal or DX12 device
    /// has no Vulkan handles to hand over.
    #[error("the graphics device the runtime wants is not the one wgpu created")]
    DeviceMismatch,
    /// The runtime said no, with its own code.
    #[error("the runtime refused with code {0}")]
    Runtime(i32),
}

impl From<openxr::sys::Result> for Unavailable {
    fn from(result: openxr::sys::Result) -> Self {
        match result {
            openxr::sys::Result::ERROR_FORM_FACTOR_UNAVAILABLE
            | openxr::sys::Result::ERROR_FORM_FACTOR_UNSUPPORTED => Self::NoHeadset,
            openxr::sys::Result::ERROR_SWAPCHAIN_FORMAT_UNSUPPORTED => Self::NoFormat,
            other => Self::Runtime(other.into_raw()),
        }
    }
}
