//! Why a device would not play.
//!
//! The seam against `device.rs` is that nothing here touches a stream: this is
//! the answer a caller gets when there is no stream to touch, and it is the
//! only part of the device half that a build without one still has an opinion
//! about.

/// Why a device would not play.
///
/// Every case here is the machine rather than the game: an `AudioFrame` cannot
/// be wrong in a way that reaches this type, because it is data.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Unavailable {
    /// The platform has no output device. A machine with no sound card, and a
    /// container without one, are both this.
    #[error("this machine has no audio output device")]
    NoDevice,
    /// A device exists and would not say what it can play.
    #[error("the device would not say what it can play: {0}")]
    NoConfig(#[source] cpal::Error),
    /// A device wants a sample format this backend does not write.
    ///
    /// The formats written are `f32`, `f64`, `i16`, `i32`, `u16` and `u8`,
    /// which is every one a desktop or a phone has offered so far. A device
    /// asking for something else is a variant to add rather than a design
    /// decision.
    #[error("the device wants {0:?} samples, which this backend does not write")]
    Unwritable(cpal::SampleFormat),
    /// The device would not open a stream.
    #[error("the device would not open a stream: {0}")]
    Unopened(#[source] cpal::Error),
    /// A stream was opened and would not start.
    #[error("the stream would not start: {0}")]
    Unstarted(#[source] cpal::Error),
}
