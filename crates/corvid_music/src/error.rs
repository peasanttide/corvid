//! What can be wrong with a song or a bank.

/// Why a song or a bank could not be read, or the synthesizer refused one.
///
/// Every case is the data rather than the machine: a machine without a sound
/// card is [`Unavailable`](crate::Unavailable), which is a different question.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The bytes are not a Standard MIDI File this reader understands.
    #[error("not a playable MIDI file: {0}")]
    Midi(#[source] rameau_midi::error::MidiError),
    /// The bytes are not a soundfont, or a sample in it will not decode.
    ///
    /// Carried as text: the loader's own error holds a decoder's error type
    /// that is neither comparable nor guaranteed to cross threads, and the one
    /// thing a caller does with this is say it.
    #[error("not a usable SoundFont: {0}")]
    SoundFont(String),
    /// The synthesizer refused an event or a render.
    #[error("the synthesizer refused: {0}")]
    Synth(String),
}
