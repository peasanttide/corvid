//! What a bar of music is, once every decision in it has been made.

use alloc::vec::Vec;

use crate::compose::{Cadence, Chord, Mode, MotifId};
use crate::num;

/// What a voice is doing in the texture.
///
/// Not an instrument. Which instrument plays a role is a data-pack decision --
/// a range, a court affinity and a set of permitted roles are records a game
/// loads -- and this crate never learns one. What it decides is that there is a
/// line here, in this range, doing this job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub enum Role {
    /// The tune. Quoted material, never bent to fit a chord.
    Lead,
    /// The root, moving as little as it can.
    Bass,
    /// A chord tone under the tune.
    Inner,
    /// Unpitched, on the dance pattern.
    Percussion,
}

/// An agrement applied to a note of the lead.
///
/// The ornament is *expanded into notes* by the composer, so a bar is playable
/// by anything that can play notes and nothing downstream has to know what a
/// port-de-voix is. The marker survives on the first note of the group so that
/// a debug overlay, or an arranger that wants to articulate the group
/// differently, can still see what the gesture was.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub enum Ornament {
    /// An alternation with the note above, reserved for cadences.
    Trill,
    /// A single flick to the note below and back.
    Mordent,
    /// A leaning appoggiatura from below.
    PortDeVoix,
    /// A passing note filling a third.
    Coule,
}

/// One sounded note.
///
/// The onset is absolute within the bar rather than a duration since the last
/// note, because a bar is read by a scheduler that wants to know when to start
/// something and by an analysis that wants to know what is sounding together.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Note {
    /// MIDI key, `0 ..= 127`.
    pub key: u8,
    /// Beats from the start of the bar to this note's onset.
    pub beat: f32,
    /// How long it sounds, in beats.
    pub beats: f32,
    /// How hard it is struck, `0 ..= 127`.
    pub velocity: u8,
    /// The gesture this note opens, when it opens one.
    pub ornament: Option<Ornament>,
}

impl Note {
    /// A note at full velocity with no ornament.
    #[must_use]
    pub const fn new(key: u8, beat: f32, beats: f32) -> Self {
        Self {
            key,
            beat,
            beats,
            velocity: 96,
            ornament: None,
        }
    }

    /// Sets how hard it is struck.
    #[must_use]
    pub const fn struck(self, velocity: u8) -> Self {
        Self { velocity, ..self }
    }

    /// The beat this note stops sounding on.
    #[must_use]
    pub fn end(&self) -> f32 {
        self.beat + self.beats
    }
}

/// One line of the texture: what it is doing, where it can go, and what it
/// plays.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Voice {
    /// What this line is doing.
    pub role: Role,
    /// Lowest key it may take.
    pub low: u8,
    /// Highest key it may take.
    pub high: u8,
    /// What it plays, in onset order. Empty means it sits the bar out, which is
    /// what a voice with no room under the tune does rather than crowd it.
    pub notes: Vec<Note>,
}

impl Voice {
    /// An empty line with the given role and range.
    #[must_use]
    pub const fn new(role: Role, low: u8, high: u8) -> Self {
        Self {
            role,
            low,
            high,
            notes: Vec::new(),
        }
    }

    /// Whether this line is pitched, which percussion is not.
    #[must_use]
    pub fn is_pitched(&self) -> bool {
        self.role != Role::Percussion
    }

    /// The key sounding at `beat`, or `None` when nothing is.
    #[must_use]
    pub fn sounding(&self, beat: f32) -> Option<u8> {
        self.notes
            .iter()
            .find(|note| note.beat <= beat + EPSILON && beat < note.end() - EPSILON)
            .map(|note| note.key)
    }

    /// The last key this line sounds in the bar.
    #[must_use]
    pub fn last_key(&self) -> Option<u8> {
        self.notes.last().map(|note| note.key)
    }
}

/// How close two beat positions have to be to count as the same moment.
///
/// A thirty-second note at the fastest tempo this crate writes is four orders of
/// magnitude above it, so nothing musical is inside the tolerance and every
/// float that should be equal is.
pub(crate) const EPSILON: f32 = 1e-4;

/// A bar of music, and every decision that produced it.
///
/// This is the composer's whole output. It carries no audio, no instrument and
/// no device: it says what is played, and something else decides what plays it.
/// The decisions are on it -- the chord, the mode, whether a cadence landed,
/// which motif is being quoted -- because a debug overlay that has to
/// re-derive them would be re-implementing the composer to explain it.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Bar {
    /// Which bar of the run this is, counting from zero.
    pub index: u32,
    /// Beats per minute.
    pub tempo: f32,
    /// How long this bar actually is, in beats. Shorter than
    /// [`beats_per_bar`](Self::beats_per_bar) when the bar was elided.
    pub beats: f32,
    /// How long a bar of this metre is, in beats.
    pub beats_per_bar: f32,
    /// Pitch class of the tonic, `0` for C.
    pub tonic: u8,
    /// The mode every degree in the bar was resolved through.
    pub mode: Mode,
    /// The chord the accompaniment is built on.
    pub chord: Chord,
    /// The cadence that landed here, or `None` -- which is what a deferred
    /// cadence looks like from outside.
    pub cadence: Option<Cadence>,
    /// The motif the lead is quoting, when it is quoting one.
    pub motif: Option<MotifId>,
    /// Which variation of that motif: `0` is the tune as the pack wrote it.
    pub variation: u32,
    /// Whether this bar was cut short by an interruption.
    pub elided: bool,
    /// The lines, lead first.
    pub voices: Vec<Voice>,
}

impl Bar {
    /// How long the bar lasts, in seconds.
    ///
    /// Zero for a tempo that is not positive, because the alternative is an
    /// infinity that propagates into every rate computed from it.
    #[must_use]
    pub fn seconds(&self) -> f32 {
        if self.tempo > 0.0 {
            self.beats * 60.0 / self.tempo
        } else {
            0.0
        }
    }

    /// How many notes start in this bar, across every line.
    #[must_use]
    pub fn onsets(&self) -> usize {
        self.voices.iter().map(|voice| voice.notes.len()).sum()
    }

    /// How many notes start per second.
    ///
    /// The number a listener hears as "how busy is this". It rises with tempo
    /// at a fixed rhythm and with rhythmic density at a fixed tempo, which is
    /// two different claims and is why the rate exists rather than the count.
    #[must_use]
    pub fn onsets_per_second(&self) -> f32 {
        let seconds = self.seconds();
        if seconds > 0.0 {
            num::of(self.onsets()) / seconds
        } else {
            0.0
        }
    }

    /// The first line with this role.
    #[must_use]
    pub fn voice(&self, role: Role) -> Option<&Voice> {
        self.voices.iter().find(|voice| voice.role == role)
    }

    /// How many pitched lines the bar has, whether or not they sound.
    #[must_use]
    pub fn pitched(&self) -> usize {
        self.voices
            .iter()
            .filter(|voice| voice.is_pitched())
            .count()
    }

    /// Whether nothing at all sounds in this bar.
    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.voices.iter().all(|voice| voice.notes.is_empty())
    }
}
