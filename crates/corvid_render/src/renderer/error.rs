//! What a device, a surface or a read-back refuses with.
//!
//! The seam is that nothing here is a game's frame being wrong. What a game
//! records is `wgpu` calls, which report their own problems through `wgpu`'s
//! device error handling, so every case here is about a device, a window, or a
//! buffer.

/// Something went wrong setting up or drawing with a device.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The window would not become a surface.
    #[error("this window did not become a surface: {0}")]
    Surface(#[source] wgpu::CreateSurfaceError),
    /// No adapter would serve. On a machine with no GPU at all this is what a
    /// missing software adapter looks like.
    #[error("no adapter would serve: {0}")]
    NoAdapter(#[source] wgpu::RequestAdapterError),
    /// An adapter was found and would not open a device.
    #[error("the adapter would not open a device: {0}")]
    NoDevice(#[source] wgpu::RequestDeviceError),
    /// The surface could not hand over a texture to draw into, and
    /// reconfiguring did not help.
    #[error("the surface has no frame to draw into: {0}")]
    NoFrame(Unacquired),
    /// A frame could not be read back off the device.
    ///
    /// The reason it carries a [`String`] rather than what `wgpu` refused
    /// with: the refusal arrives on a channel as a type this crate cannot name
    /// in its own signature, and the sentence is what a reader wants anyway.
    #[error("the frame could not be read back: {0}")]
    NotRead(String),
    /// [`Renderer::read_back`](crate::Renderer::read_back) was called on a
    /// renderer that draws into a window. A window's frame belongs to the
    /// compositor once it is presented, so there is nothing left here to read.
    #[error(
        "this renderer draws into a window, and a presented frame belongs to the compositor \
         rather than to us"
    )]
    NotOffscreen,
}

/// Why a surface would not hand over a texture to draw into.
///
/// The two a caller can do something about -- a surface that has gone out of
/// date and one that has been lost -- are handled inside
/// [`Renderer::frame`](crate::Renderer::frame) by configuring again, so
/// reaching this type means the second attempt failed too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum Unacquired {
    /// The surface configuration no longer matches the window.
    #[error("the surface configuration no longer matches the window")]
    Outdated,
    /// The surface itself has to be built again, which needs the window back.
    #[error("the surface has been lost and has to be built again from the window")]
    Lost,
    /// A validation error was raised inside the request.
    #[error("the request for a surface texture raised a validation error")]
    Validation,
}
