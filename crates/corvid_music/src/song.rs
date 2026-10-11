//! A Standard MIDI File, read once into the timeline a synthesizer plays.

use std::sync::Arc;

use rameau_midi::event::MidiEvent;
use rameau_midi::smf::Smf;

use crate::Error;

/// A piece of music: every playable event of a Standard MIDI File, merged
/// across its tracks and placed in seconds from its start.
///
/// The tempo map is walked once, here, so playing a song is reading a sorted
/// list rather than interpreting a file. A song is cheap to clone -- the
/// timeline is shared -- so a game keeps its soundtrack parsed and hands a
/// clone to the player each time one starts.
///
/// ```
/// use corvid_music::Song;
///
/// // Not a MIDI file at all.
/// assert!(Song::parse(b"RIFF").is_err());
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    /// `(seconds from the start, event)`, sorted by time.
    events: Arc<[(f64, MidiEvent)]>,
}

impl Song {
    /// Reads the bytes of a Standard MIDI File (format 0 or 1).
    ///
    /// # Errors
    ///
    /// [`Error::Midi`] when the bytes are not a well-formed Standard MIDI File.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let smf = Smf::parse(bytes).map_err(Error::Midi)?;
        Ok(Self {
            events: smf.timed_events().into(),
        })
    }

    /// How long the song is, in seconds: the time of its last event.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        self.events.last().map_or(0.0, |&(at, _)| at)
    }

    /// How many playable events it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether it holds no playable event at all, which plays as silence.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The event at `index` of the timeline, and when it falls.
    pub(crate) fn event(&self, index: usize) -> Option<(f64, MidiEvent)> {
        self.events.get(index).copied()
    }
}
