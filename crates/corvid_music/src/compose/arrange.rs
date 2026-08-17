//! Laying the lines out under the tune.
//!
//! Which roles there are, which range each of them plays in, and what each of
//! them plays -- one bar's worth, given a chord and the tune it has to stay
//! under. The counterpoint search and its promise about parallels come after
//! this and are in [`search`](crate::compose::search); what is here is the
//! writing that search is an improvement on.

use alloc::vec::Vec;

use crate::compose::{Note, Parameters, Role, Voice, voicing};
use crate::num;

/// The velocity the bass is struck at.
const BASS_VELOCITY: u8 = 80;
/// The velocity an inner line is struck at.
const INNER_VELOCITY: u8 = 68;
/// The grit above which there is percussion at all.
const DRUM_THRESHOLD: f32 = 0.35;

/// Where the previous bar left the accompaniment.
///
/// Voice leading is a claim about the join between two bars, so writing this one
/// needs where the last one ended. Absent for the first bar of a run, where
/// every line starts from the middle of its range instead.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Left {
    /// The bass's last key.
    pub(crate) bass: Option<u8>,
    /// Each inner line's key, in the order the lines are written.
    pub(crate) inner: Vec<u8>,
}

/// Writes every line of a bar: the tune as given, and the accompaniment under
/// it.
///
/// `classes` is the chord as pitch classes and `ceiling` is the key the
/// accompaniment must stay below, which is [`floor_of`] the tune. Percussion is
/// appended last and is the only unpitched line, which is what lets everything
/// downstream index the pitched lines by position.
pub(crate) fn lines(
    parameters: Parameters,
    beats: f32,
    classes: &[u8],
    ceiling: u8,
    tune: Vec<Note>,
    left: &Left,
) -> Vec<Voice> {
    let figure = voicing::Figure::of(parameters.density, parameters.syncopation);
    let lead_range = voicing::range(Role::Lead, 0, parameters.register);
    let mut voices = Vec::new();
    let mut lead = Voice::new(Role::Lead, lead_range.0, lead_range.1);
    lead.notes = tune;
    voices.push(lead);

    if parameters.voices >= 2 {
        let (low, high) = voicing::range(Role::Bass, 0, parameters.register);
        let mut bass = Voice::new(Role::Bass, low, high);
        bass.notes = voicing::bass(
            classes,
            figure,
            beats,
            low,
            high.min(ceiling),
            left.bass,
            BASS_VELOCITY,
        );
        voices.push(bass);
    }

    // Every line past the second is an inner one, laid out a little higher than
    // the one before it so that four of them are four lines rather than four
    // attempts at the same one.
    let inner = usize::from(parameters.voices.saturating_sub(2));
    let mut taken: Vec<u8> = Vec::new();
    for index in 0..inner {
        let (low, high) = voicing::range(Role::Inner, index, parameters.register);
        let mut voice = Voice::new(Role::Inner, low, high);
        voice.notes = voicing::inner(
            classes,
            figure,
            beats,
            (low, high.min(ceiling)),
            left.inner.get(index).copied(),
            &taken,
            INNER_VELOCITY,
        );
        if let Some(note) = voice.notes.first() {
            taken.push(note.key);
        }
        voices.push(voice);
    }

    if parameters.grit > DRUM_THRESHOLD {
        let mut drum = Voice::new(Role::Percussion, voicing::DRUM_KEY, voicing::DRUM_KEY);
        drum.notes = voicing::percussion(beats, parameters.grit, parameters.syncopation);
        voices.push(drum);
    }
    voices
}

/// The key the accompaniment must stay under.
///
/// One low note in the tune should not crush the accompaniment down into it, so
/// what is used is the higher of the tune's lowest note and five semitones below
/// its average. `None` for a bar with no tune in it, where the accompaniment has
/// the whole of its own range.
pub(crate) fn floor_of(tune: &[Note]) -> Option<u8> {
    let lowest = tune.iter().map(|note| note.key).min()?;
    let mean = tune.iter().map(|note| f32::from(note.key)).sum::<f32>() / num::of(tune.len());
    Some(lowest.max(num::key(mean - 5.0)))
}

/// What the accompaniment of `voices` leaves for the bar after it.
pub(crate) fn left_by(voices: &[Voice]) -> Left {
    Left {
        bass: voices
            .iter()
            .find(|voice| voice.role == Role::Bass)
            .and_then(Voice::last_key),
        inner: voices
            .iter()
            .filter(|voice| voice.role == Role::Inner)
            .map(|voice| voice.notes.first().map_or(0, |note| note.key))
            .collect(),
    }
}
