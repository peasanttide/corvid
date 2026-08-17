//! The tune: quoting it a bar at a time, and putting it where it sings.
//!
//! The melody is the one thing the composer never negotiates. It is quoted
//! material, it is written out exactly as the pack recorded it, and the chord is
//! chosen to fit it rather than the other way round. A tune annealed into notes
//! that are in key, on chord tones and no longer the tune is the failure every
//! rule in this module exists to prevent.

use alloc::vec::Vec;

use crate::compose::{Event, Mode, Note};
use crate::num;

/// A place in a motif, kept across bars.
///
/// A motif is rarely a whole number of bars long, so the cursor has to remember
/// which event it is inside and how much of that event has already sounded.
/// Reaching the end wraps and counts a variation, which is the moment the
/// composer applies a new transformation: the tune you hear is the tune,
/// transformed on purpose, never by random search.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Quote {
    /// Which event is next.
    pub(crate) cursor: usize,
    /// How much of it has already sounded, in beats.
    pub(crate) used: f32,
    /// How many times the motif has been round.
    pub(crate) laps: u32,
}

impl Quote {
    /// Takes the next `beats` beats of `events`, wrapping as often as needed.
    ///
    /// Answers events whose durations sum to `beats` exactly, so a bar is always
    /// full: an event that runs past the end of the bar is split, and its
    /// remainder is where the next bar starts. An empty `events` answers a
    /// single rest, because a bar of a phrase with nothing to quote is still a
    /// bar.
    pub(crate) fn take(&mut self, events: &[Event], beats: f32) -> Vec<Event> {
        let mut out = Vec::new();
        if events.is_empty() || beats <= 0.0 {
            out.push(Event::rest(beats.max(0.0)));
            return out;
        }
        let mut needed = beats;
        // One guard step per emitted event, bounded well above any real bar so
        // that a motif made entirely of zero-length events ends rather than
        // spins.
        for _ in 0..1024 {
            if needed <= 1e-4 {
                break;
            }
            let Some(event) = events.get(self.cursor) else {
                self.cursor = 0;
                self.used = 0.0;
                self.laps = self.laps.saturating_add(1);
                continue;
            };
            let left = (event.beats - self.used).max(0.0);
            if left <= 1e-4 {
                self.cursor += 1;
                self.used = 0.0;
                continue;
            }
            let taken = left.min(needed);
            out.push(Event {
                step: event.step,
                beats: taken,
            });
            needed -= taken;
            self.used += taken;
            if self.used >= event.beats - 1e-4 {
                self.cursor += 1;
                self.used = 0.0;
            }
        }
        if needed > 1e-4 {
            out.push(Event::rest(needed));
        }
        out
    }
}

/// Where in its range a line sits at `register`.
const REGISTER_LOW: f32 = 0.38;
/// How much of the range register moves the line through.
const REGISTER_SPAN: f32 = 0.24;

/// Chooses the octave that puts `events` where the range sings, and writes them
/// out as notes.
///
/// The octave is chosen once for the whole bar rather than note by note, because
/// a tune that hops an octave in the middle of itself is a different tune.
/// `previous` is last bar's choice: a bar that would only just prefer to move by
/// one octave keeps the old one instead, so the line does not oscillate between
/// two nearly equal answers at every barline.
pub(crate) fn place(
    events: &[Event],
    tonic: u8,
    mode: Mode,
    range: (u8, u8),
    register: f32,
    previous: Option<i8>,
) -> (i8, Vec<Note>) {
    let (low, high) = range;
    let span = f32::from(high.saturating_sub(low));
    let target = f32::from(low) + span * (REGISTER_LOW + REGISTER_SPAN * register.clamp(0.0, 1.0));

    let mut best: Option<(f32, i8)> = None;
    for octave in 1i8..=8 {
        let keys: Vec<u8> = events
            .iter()
            .filter_map(|event| event.step)
            .map(|step| step.key(tonic, mode, octave))
            .collect();
        if keys.is_empty() {
            break;
        }
        let mean = keys.iter().map(|key| f32::from(*key)).sum::<f32>() / num::of(keys.len());
        let over: f32 = keys
            .iter()
            .map(|key| {
                let key = f32::from(*key);
                (f32::from(low) - key).max(0.0) + (key - f32::from(high)).max(0.0)
            })
            .sum();
        let error = libm::fabsf(mean - target) + over * 2.0;
        if best.is_none_or(|(held, _)| error < held) {
            best = Some((error, octave));
        }
    }
    let (error, mut octave) = best.unwrap_or((0.0, 4));
    if let Some(previous) = previous
        && (previous - octave).abs() == 1
        && error > 2.0
    {
        octave = previous;
    }

    let mut notes = Vec::new();
    let mut at = 0.0;
    for event in events {
        if let Some(step) = event.step {
            notes.push(Note::new(step.key(tonic, mode, octave), at, event.beats));
        }
        at += event.beats;
    }
    (octave, notes)
}
