//! One-shots, and the identity a rollback makes them need.

use core::fmt;

use crate::{BusId, SoundId};
use corvid_fixed::{Factor16, I8F8};
use corvid_time::Tick;
use corvid_vector::FinePoint;

/// Which one-shot this is: the tick it was fired on, and its place among the
/// cues fired on that tick.
///
/// # The problem this exists to solve
///
/// A cue is fired by the simulation and observed by the client. Those two
/// happen at different rates and, under rollback, in a relationship that is not
/// a function.
///
/// The simulation is authoritative and re-runnable. When a late or corrected
/// action arrives for tick 95, a runtime rewinds to the last state at or before
/// 95 and re-simulates forward. The states it produces for 95 through
/// 100 the second time need not be the states it produced the first time — that
/// is the entire point of doing it. So a bounce that happened on tick 97 in the
/// first pass may not happen in the second, and a bounce that did not happen may
/// now happen.
///
/// Sound does not rewind. By the time the correction arrives, the thud from tick
/// 97 has already left the speaker. A rollback can therefore **un-fire a cue
/// that has already been played**, and can **re-fire one that was already
/// played** — and a mixer holding a list of voices it has started has to tell
/// those two apart from a third case that looks identical from the outside: two
/// genuinely different cues that happen to carry the same sound, gain and
/// position.
///
/// Nothing about the payload can settle that. The client extracts an audio
/// frame once per *displayed* frame, and a fifteen-hertz simulation on a
/// hundred-and-forty-four-hertz display is extracted from nine or ten times per
/// tick. Between two of those extractions the listener has moved, so the same
/// cue's position — which is an offset in the listener's frame — is a different
/// number. Comparing payloads would call one cue two, every frame, forever.
///
/// So identity is `(fired, serial)` and is deliberately **disjoint from the
/// payload**: the tick the simulation fired it on, and which of that tick's
/// cues it is. Both are functions of the simulation alone, so re-running ticks
/// 95 through 100 produces the same identities for the same events, whatever
/// the camera did in between.
///
/// # What a mixer still has to decide
///
/// This crate has no mixer, and nothing here can show that this identity is
/// *sufficient* for one — only that it is stable across two observations of one
/// cue and distinct across ticks and across serials, which `tests/cue.rs`
/// checks. Three decisions are left open, and a mixer must make all three:
///
/// An identity that **disappears** from the frame after a rollback names a
/// sound that was played and should not have been. Cutting it is abrupt,
/// letting it ring out is a lie, and ducking it is a compromise; which is right
/// depends on the sound.
///
/// An identity that **reappears** having already been started must not be
/// started twice — but if the re-simulation produced a different payload under
/// the same identity, the mixer holds a voice that is playing the wrong thing.
/// [`Cue`]'s `PartialEq` compares the whole value, identity included, so
/// detecting that is a comparison; deciding between restarting, retuning and
/// ignoring is not.
///
/// And a cue whose identity the mixer has never seen may be new, or may be one
/// it has already finished playing and forgotten. Nothing in the frame says
/// which. How long a mixer remembers is a memory budget, and this crate does not
/// set it.
///
/// # Serials
///
/// The serial is a position, not a name: it is assigned by
/// [`AudioFrame::next_id`](crate::AudioFrame::next_id) from what is already in
/// the frame. Two extractions of the same ticks agree only if the extractor
/// emits its cues in the same order both times, which is an obligation on the
/// extractor — iterating a `BTreeMap` keyed by an entity identifier keeps it,
/// and iterating a `HashMap` does not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct CueId {
    /// The tick the simulation fired it on.
    pub fired: Tick,
    /// Which of that tick's cues this is, counting from zero.
    pub serial: u16,
}

impl CueId {
    /// The first cue of `fired`.
    #[must_use]
    #[inline]
    pub const fn first(fired: Tick) -> Self {
        Self { fired, serial: 0 }
    }

    /// Builds one directly, for a reader of a captured frame that already knows
    /// both halves.
    #[must_use]
    #[inline]
    pub const fn new(fired: Tick, serial: u16) -> Self {
        Self { fired, serial }
    }
}

impl fmt::Display for CueId {
    /// `97#0` — the tick, a hash, the serial.
    ///
    /// For reading, not for sorting. Both halves are rendered without padding,
    /// so a lexical sort of these strings disagrees with [`Ord`] wherever two
    /// numbers in the same position have different lengths: `97#2` and `97#10`
    /// are one apart in order and the other way round as text, and so are `9#0`
    /// and `10#0`. Sort the identities and format the result.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.fired, self.serial)
    }
}

/// A one-shot: a sound fired at a moment, rather than one that is playing.
///
/// The split from [`Source`](crate::Source) is not cosmetic. A source is a
/// thing the frame keeps describing for as long as it is audible, and a cue is
/// an event the frame mentions once — which is why a cue carries a [`CueId`]
/// and a source carries a [`SourceId`](crate::SourceId): the first says *which
/// event*, and the second says *which voice*.
///
/// The position is an offset in the listener's frame, as it is for a source.
///
/// ```
/// use corvid_sound::{Cue, CueId, SoundId};
/// use corvid_time::Tick;
///
/// const THUD: SoundId = SoundId(1);
///
/// let thud = Cue::new(CueId::new(Tick(97), 0), THUD);
/// assert_eq!(thud.id.fired, Tick(97));
/// assert_eq!(thud.id.to_string(), "97#0");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Cue {
    /// Which one-shot this is. See [`CueId`] for why it is not the payload.
    pub id: CueId,
    /// Which recording to play.
    pub sound: SoundId,
    /// Which bus it is routed through.
    pub bus: BusId,
    /// Where it happened, as an offset in the listener's frame.
    pub position: FinePoint,
    /// How loud, before distance and before the bus.
    pub gain: Factor16,
    /// Playback rate, where [`I8F8::ONE`] is the recorded rate.
    pub pitch: I8F8,
}

impl Cue {
    /// A cue at the listener, at full gain, on the master bus, playing at the
    /// recorded rate.
    #[must_use]
    #[inline]
    pub const fn new(id: CueId, sound: SoundId) -> Self {
        Self {
            id,
            sound,
            bus: BusId::MASTER,
            position: FinePoint::ZERO,
            gain: Factor16::ONE,
            pitch: I8F8::ONE,
        }
    }

    /// Places it, as an offset in the listener's frame.
    #[must_use]
    #[inline]
    pub const fn at(self, position: FinePoint) -> Self {
        Self { position, ..self }
    }

    /// Routes it through `bus`.
    #[must_use]
    #[inline]
    pub const fn on(self, bus: BusId) -> Self {
        Self { bus, ..self }
    }

    /// Sets the gain.
    #[must_use]
    #[inline]
    pub const fn with_gain(self, gain: Factor16) -> Self {
        Self { gain, ..self }
    }

    /// Sets the playback rate.
    #[must_use]
    #[inline]
    pub const fn with_pitch(self, pitch: I8F8) -> Self {
        Self { pitch, ..self }
    }
}
