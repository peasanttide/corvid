//! Where each line sits, and what it plays under the tune.
//!
//! Constructed correct rather than searched for. The bass takes the root and
//! moves as little as it can; an inner voice takes a chord tone as near as it
//! can get to where it was last bar, under the melody; a voice with no room
//! under the melody sits the bar out rather than crowding it. The search in
//! [`crate::compose::cost`] then improves on that, and can only ever choose a
//! different chord tone or a different octave -- so whatever it does, the result
//! is still in the chord.

use alloc::vec::Vec;

use crate::compose::{Chord, Mode, Note, Role};
use crate::num;

/// The lowest and highest key of the lead's ordinary range, before register.
const LEAD_RANGE: (u8, u8) = (60, 84);
/// The same for the bass.
const BASS_RANGE: (u8, u8) = (36, 60);
/// The bottom of the first inner voice's range, and how far each further inner
/// voice sits above the one before it.
const INNER_FLOOR: u8 = 48;
/// How far apart successive inner voices are laid out.
const INNER_STEP: u8 = 4;
/// How wide an inner voice's range is.
const INNER_WIDTH: u8 = 26;

/// How far `register` may shift a range, in semitones either way.
const REGISTER_SHIFT: f32 = 7.0;

/// The range a line of this role and index plays in.
///
/// Register moves the whole texture rather than only the tune, because a
/// faubourg that sings lower sings lower in every part; the bass moves less
/// than the lead, because there is less room below it.
#[must_use]
pub(crate) fn range(role: Role, index: usize, register: f32) -> (u8, u8) {
    let shift = (register.clamp(0.0, 1.0) - 0.5) * 2.0 * REGISTER_SHIFT;
    let moved = |bound: u8, by: f32| num::key(f32::from(bound) + by);
    match role {
        Role::Lead => (moved(LEAD_RANGE.0, shift), moved(LEAD_RANGE.1, shift)),
        Role::Bass => (
            moved(BASS_RANGE.0, shift * 0.4),
            moved(BASS_RANGE.1, shift * 0.4),
        ),
        Role::Inner => {
            let step = u8::try_from(index.min(4)).unwrap_or(0) * INNER_STEP;
            let low = moved(INNER_FLOOR.saturating_add(step), shift * 0.7);
            (low, moved(low.saturating_add(INNER_WIDTH), 0.0))
        }
        Role::Percussion => (0, 0),
    }
}

/// The pitch classes `chord` is made of, root first.
#[must_use]
pub(crate) fn pitch_classes(chord: Chord, tonic: u8, mode: Mode) -> Vec<u8> {
    let scale = mode.semitones();
    chord
        .tones()
        .map(|degree| {
            let semitone = scale.get(usize::from(degree)).copied().unwrap_or(0);
            u8::try_from((i16::from(tonic % 12) + i16::from(semitone)).rem_euclid(12)).unwrap_or(0)
        })
        .collect()
}

/// The key of pitch class `class` nearest `target`, inside `low ..= high`.
///
/// `None` when the range holds no key of that class at all, which happens to an
/// inner voice squeezed into fewer than twelve semitones under the tune.
#[must_use]
pub(crate) fn nearest(class: u8, target: f32, low: u8, high: u8) -> Option<u8> {
    if low > high {
        return None;
    }
    (low..=high)
        .filter(|key| key % 12 == class % 12)
        .min_by(|left, right| {
            let distance = |key: u8| libm::fabsf(f32::from(key) - target);
            distance(*left)
                .partial_cmp(&distance(*right))
                .unwrap_or(core::cmp::Ordering::Equal)
        })
}

/// A rhythmic figure the accompaniment plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Figure {
    /// One note the length of the bar.
    Sustain,
    /// Two, half a bar each.
    Half,
    /// One a beat.
    Pulse,
    /// Two a beat, the first silent.
    Offbeat,
    /// Two a beat, both sounding.
    Arpeggio,
}

impl Figure {
    /// The figure `density` asks for, with `syncopation` deciding between the
    /// two that are equally busy.
    pub(crate) fn of(density: f32, syncopation: f32) -> Self {
        if density > 0.72 {
            Self::Arpeggio
        } else if density > 0.45 {
            if syncopation > 0.5 {
                Self::Offbeat
            } else {
                Self::Half
            }
        } else if density > 0.22 {
            Self::Pulse
        } else {
            Self::Sustain
        }
    }

    /// The onsets and durations this figure fills `beats` with.
    pub(crate) fn beats(self, beats: f32) -> Vec<(f32, f32)> {
        let beats = beats.max(0.0);
        let mut out = Vec::new();
        let mut push = |at: f32, length: f32| {
            let length = length.min(beats - at);
            if at < beats - 1e-4 && length > 1e-4 {
                out.push((at, length));
            }
        };
        match self {
            Self::Sustain => push(0.0, beats),
            Self::Half => {
                push(0.0, beats / 2.0);
                push(beats / 2.0, beats / 2.0);
            }
            Self::Pulse | Self::Offbeat | Self::Arpeggio => {
                let whole = num::count(libm::ceilf(beats));
                for index in 0..whole {
                    let at = num::of(index);
                    match self {
                        Self::Offbeat => push(at + 0.5, 0.5),
                        Self::Arpeggio => {
                            push(at, 0.5);
                            push(at + 0.5, 0.5);
                        }
                        _ => push(at, 1.0),
                    }
                }
            }
        }
        out
    }
}

/// Writes the bass: the root, moving as little as it can, with the fifth on
/// every second note of a figure that has more than one.
///
/// `previous` is where the bass was left last bar, and is what "as little as it
/// can" is measured from.
pub(crate) fn bass(
    classes: &[u8],
    figure: Figure,
    beats: f32,
    low: u8,
    high: u8,
    previous: Option<u8>,
    velocity: u8,
) -> Vec<Note> {
    let Some(root_class) = classes.first().copied() else {
        return Vec::new();
    };
    let target = previous.map_or_else(|| f32::from(low) + 8.0, f32::from);
    let Some(root) = nearest(root_class, target, low, high) else {
        return Vec::new();
    };
    let fifth = classes
        .get(2)
        .copied()
        .and_then(|class| nearest(class, f32::from(root), low, high));
    figure
        .beats(beats)
        .into_iter()
        .enumerate()
        .map(|(index, (at, length))| {
            let key = match fifth {
                Some(fifth) if index % 2 == 1 && figure != Figure::Sustain => fifth,
                _ => root,
            };
            Note::new(key, at, length).struck(velocity)
        })
        .collect()
}

/// Writes one inner voice: a chord tone as near as it can get to where it was,
/// and under `ceiling`.
///
/// `taken` is what the inner voices already written took, so two of them do not
/// land on the same key and turn three lines into two. Answers an empty line
/// when the range under the tune is too narrow to hold a chord tone at all,
/// which is the "sits the bar out" case.
pub(crate) fn inner(
    classes: &[u8],
    figure: Figure,
    beats: f32,
    range: (u8, u8),
    previous: Option<u8>,
    taken: &[u8],
    velocity: u8,
) -> Vec<Note> {
    let (low, ceiling) = range;
    if ceiling <= low {
        return Vec::new();
    }
    let target = previous.map_or_else(
        || f32::midpoint(f32::from(low), f32::from(ceiling)),
        f32::from,
    );
    // The third and the seventh carry the harmony, so they are tried before the
    // fifth and the root: doubling a root is the least informative thing an
    // inner voice can do.
    let order = [1usize, 3, 2, 0];
    let mut best: Option<(f32, u8)> = None;
    for index in order {
        let Some(class) = classes.get(index).copied() else {
            continue;
        };
        let Some(key) = nearest(class, target, low, ceiling) else {
            continue;
        };
        let penalty = if taken.contains(&key) { 9.0 } else { 0.0 };
        let cost = libm::fabsf(f32::from(key) - target) + penalty;
        if best.is_none_or(|(held, _)| cost < held) {
            best = Some((cost, key));
        }
    }
    let Some((_, key)) = best else {
        return Vec::new();
    };
    figure
        .beats(beats)
        .into_iter()
        .map(|(at, length)| Note::new(key, at, length).struck(velocity))
        .collect()
}

/// Writes the percussion: an accent on the downbeat and a hit on every beat
/// after it, at a velocity `grit` sets.
pub(crate) fn percussion(beats: f32, grit: f32, syncopation: f32) -> Vec<Note> {
    let grit = grit.clamp(0.0, 1.0);
    let accent = num::key(64.0 + grit * 56.0);
    let quiet = num::key(40.0 + grit * 40.0);
    let mut notes = Vec::new();
    let whole = num::count(libm::ceilf(beats.max(0.0)));
    for index in 0..whole {
        let at = num::of(index);
        if at >= beats {
            break;
        }
        let velocity = if index == 0 { accent } else { quiet };
        notes.push(Note::new(DRUM_KEY, at, 0.5_f32.min(beats - at)).struck(velocity));
        if syncopation > 0.55 && at + 0.5 < beats {
            notes.push(Note::new(DRUM_KEY, at + 0.5, 0.25).struck(quiet / 2));
        }
    }
    notes
}

/// The key percussion is written on.
///
/// Percussion is unpitched, so the number is a name rather than a pitch: 38 is
/// the General MIDI acoustic snare, which is what a bank is most likely to have
/// something at. A game with its own bank remaps it.
pub(crate) const DRUM_KEY: u8 = 38;
