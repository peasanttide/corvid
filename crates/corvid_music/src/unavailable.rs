//! Why the music would not play on this machine.

/// Why no sound card would take the music.
///
/// Every case is the machine rather than the song: a run that gets one plays
/// on in silence, and a game says so once rather than stopping.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Unavailable {
    /// The platform has no output device.
    #[error("this machine has no audio output device")]
    NoDevice,
    /// A device exists and would not say what it can play.
    #[error("the device would not say what it can play: {0}")]
    NoConfig(#[source] cpal::Error),
    /// A device wants a sample format this backend does not write.
    #[error("the device wants {0:?} samples, which this backend does not write")]
    Unwritable(cpal::SampleFormat),
    /// The device would not open a stream.
    #[error("the device would not open a stream: {0}")]
    Unopened(#[source] cpal::Error),
    /// A stream was opened and would not start.
    #[error("the stream would not start: {0}")]
    Unstarted(#[source] cpal::Error),
    /// The thread that plays the music could not be started, or stopped
    /// before it said whether the device opened.
    #[error("the music thread did not start")]
    NoThread,
}
