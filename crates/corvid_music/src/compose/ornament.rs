//! The agrements, applied to the lead and to nothing else.
//!
//! Decoration is the first thing to go when a street stops being polite, so the
//! rate is a parameter and the trill is reserved for a cadence. Each ornament is
//! expanded into real notes rather than left as a flag, so anything that can
//! play a note can play the gesture; the flag survives on the first note of the
//! group for a debug overlay and for an arranger that wants to articulate it
//! itself.

use alloc::vec::Vec;

use crate::compose::{Mode, Note, Ornament};
use crate::rng::Rng;

/// The shortest note worth decorating, in beats.
///
/// Below this the ornament's own notes are shorter than a player can articulate
/// and shorter than a listener can hear as separate, so the decoration would be
/// a smear rather than a gesture.
const SHORTEST: f32 = 0.5;

/// The key of the next scale note above or below `key`.
///
/// Answers `key` itself when the search finds nothing, which cannot happen for a
/// diatonic mode -- every degree has a neighbour within two semitones -- but is
/// the honest answer rather than a panic if one ever did.
fn neighbour(key: u8, tonic: u8, mode: Mode, above: bool) -> u8 {
    for distance in 1i16..=3 {
        let step = if above { distance } else { -distance };
        let candidate = i16::from(key) + step;
        if !(0..=127).contains(&candidate) {
            break;
        }
        let semitone =
            i8::try_from((candidate - i16::from(tonic % 12)).rem_euclid(12)).unwrap_or(0);
        if mode.contains_semitone(semitone) {
            return u8::try_from(candidate).unwrap_or(key);
        }
    }
    key
}

/// Expands one note into the notes an ornament is made of.
///
/// `leading` makes the note a port-de-voix leans from a semitone below rather
/// than a scale step below, which is the period's own chromatic inflection and
/// the one place a note outside the mode is written on purpose.
fn expand(note: Note, kind: Ornament, tonic: u8, mode: Mode, leading: bool) -> Vec<Note> {
    let above = neighbour(note.key, tonic, mode, true);
    let below = if leading && kind == Ornament::PortDeVoix {
        note.key.saturating_sub(1)
    } else {
        neighbour(note.key, tonic, mode, false)
    };
    let at = |offset: f32, share: f32, key: u8| {
        Note::new(key, note.beat + note.beats * offset, note.beats * share).struck(note.velocity)
    };
    let mut group = match kind {
        Ornament::Trill => alloc::vec![
            at(0.0, 0.25, note.key),
            at(0.25, 0.25, above),
            at(0.5, 0.25, note.key),
            at(0.75, 0.25, above),
        ],
        Ornament::Mordent => alloc::vec![
            at(0.0, 0.15, note.key),
            at(0.15, 0.15, below),
            at(0.3, 0.7, note.key),
        ],
        Ornament::PortDeVoix => alloc::vec![at(0.0, 0.25, below), at(0.25, 0.75, note.key)],
        Ornament::Coule => alloc::vec![at(0.0, 0.5, note.key), at(0.5, 0.5, below)],
    };
    if let Some(first) = group.first_mut() {
        first.ornament = Some(kind);
    }
    group
}

/// Decorates `notes` in place at `rate`, reserving the trill for `cadence`.
///
/// The trill lands on the last note of a cadence bar and nowhere else, which is
/// the period's own rule and is also what makes a cadence audible as one when
/// the harmony alone would be ambiguous.
pub(crate) fn decorate(
    notes: &mut Vec<Note>,
    rate: f32,
    cadence: bool,
    scale: (u8, Mode),
    chromaticism: f32,
    rng: &mut Rng,
) {
    let (tonic, mode) = scale;
    let rate = rate.clamp(0.0, 1.0);
    if rate <= 0.0 || notes.is_empty() {
        return;
    }
    let last = notes.len() - 1;
    let mut out: Vec<Note> = Vec::with_capacity(notes.len());
    for (index, note) in notes.iter().enumerate() {
        if note.beats < SHORTEST {
            out.push(*note);
            continue;
        }
        let closing = cadence && index == last;
        if closing && rng.chance(rate) {
            out.extend(expand(*note, Ornament::Trill, tonic, mode, false));
            continue;
        }
        if !rng.chance(rate) {
            out.push(*note);
            continue;
        }
        let kind = match rng.below(3) {
            Some(0) => Ornament::Mordent,
            Some(1) => Ornament::PortDeVoix,
            _ => Ornament::Coule,
        };
        let leading = rng.chance(chromaticism);
        out.extend(expand(*note, kind, tonic, mode, leading));
    }
    *notes = out;
}
