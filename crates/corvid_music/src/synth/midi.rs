//! The MIDI vocabulary, as data.
//!
//! Enough of it to sound a score and no more: this is a synthesizer's input,
//! not a sequencer's file format. Everything is a channel-voice message with
//! its data already unpacked, so nothing downstream parses a status byte.

/// One MIDI message.
///
/// Values are already unpacked: a key is `0 ..= 127` and a bend is `0 ..= 16383`
/// with `8192` at rest, so a caller never assembles a status byte and a reader
/// never has to know that a note-off is a note-on at velocity zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub enum MidiEvent {
    /// Start a note.
    NoteOn {
        /// Which channel, `0 ..= 15`.
        channel: u8,
        /// Which key, `0 ..= 127`.
        key: u8,
        /// How hard, `1 ..= 127`. A velocity of zero is a note-off, and is
        /// turned into one when it arrives.
        velocity: u8,
    },
    /// Release a note, letting it ring out.
    NoteOff {
        /// Which channel.
        channel: u8,
        /// Which key.
        key: u8,
    },
    /// Choose the preset a channel plays.
    ProgramChange {
        /// Which channel.
        channel: u8,
        /// Which bank, from the two bank-select controllers.
        bank: u16,
        /// Which program, `0 ..= 127`.
        program: u8,
    },
    /// Set a continuous controller.
    ControlChange {
        /// Which channel.
        channel: u8,
        /// Which controller, `0 ..= 127`.
        control: u8,
        /// Its new value, `0 ..= 127`.
        value: u8,
    },
    /// Bend a channel's pitch. `8192` is no bend.
    PitchBend {
        /// Which channel.
        channel: u8,
        /// The bend, `0 ..= 16383`.
        value: u16,
    },
    /// Release every note on a channel, letting them ring out.
    AllNotesOff {
        /// Which channel.
        channel: u8,
    },
    /// Stop every note on a channel at once, with no release.
    AllSoundOff {
        /// Which channel.
        channel: u8,
    },
}

impl MidiEvent {
    /// Which channel this message is for.
    #[must_use]
    pub const fn channel(self) -> u8 {
        match self {
            Self::NoteOn { channel, .. }
            | Self::NoteOff { channel, .. }
            | Self::ProgramChange { channel, .. }
            | Self::ControlChange { channel, .. }
            | Self::PitchBend { channel, .. }
            | Self::AllNotesOff { channel }
            | Self::AllSoundOff { channel } => channel,
        }
    }
}

/// The controller number for the volume a channel plays at.
pub(crate) const VOLUME: u8 = 7;
/// The controller number for the pan a channel plays at.
pub(crate) const PAN: u8 = 10;
/// The controller number for expression, which multiplies volume.
pub(crate) const EXPRESSION: u8 = 11;
/// The controller number for the coarse half of a bank select.
pub(crate) const BANK_SELECT_COARSE: u8 = 0;
/// The controller number for the fine half of a bank select.
pub(crate) const BANK_SELECT_FINE: u8 = 32;

/// A message and the frame it happens on.
///
/// The frame is absolute on the synthesizer's own clock, which is what lets a
/// whole bar be handed over at once and land in time rather than being fed in
/// block by block by whoever owns the audio callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct TimedEvent {
    /// The frame it happens on.
    pub frame: u64,
    /// What happens.
    pub event: MidiEvent,
}

impl TimedEvent {
    /// A message at `frame`.
    #[must_use]
    pub const fn new(frame: u64, event: MidiEvent) -> Self {
        Self { frame, event }
    }
}
