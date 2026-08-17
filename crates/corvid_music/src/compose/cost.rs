//! What a bar costs, under rules whose weights the parameters move.
//!
//! Destruction buys permission to break the rules, and this is where that is
//! spent. Every rule that is a matter of taste is scaled by
//! `1 - 0.7 * dissonance`, so counterpoint dissolves in an order a listener can
//! follow: the parallels creep in first, then the spacing opens up, then the
//! inner voices stop caring where they were. Two rules are never scaled. The
//! melody is never obscured, and no voice is ever written outside its range.

use alloc::vec::Vec;

use crate::compose::analysis::Grid;
use crate::compose::{Bar, Parameters, Role, Voice};
use crate::num;

/// What each rule is worth in this bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Weights {
    /// Fifths and octaves moving together.
    pub(crate) parallels: f32,
    /// A voice sounding on the wrong side of the one below it.
    pub(crate) crossing: f32,
    /// More than an octave between neighbouring upper voices.
    pub(crate) spacing: f32,
    /// An accompaniment voice moving further than it needs to.
    pub(crate) leaps: f32,
    /// Anything crowding, grinding against or sitting above the tune.
    pub(crate) melody_clash: f32,
    /// A note outside the line's range.
    pub(crate) range: f32,
    /// An accompaniment voice on the same key as the tune.
    pub(crate) doubling: f32,
}

impl Weights {
    /// The weights `parameters` asks for.
    pub(crate) fn of(parameters: Parameters) -> Self {
        let strict = 1.0 - 0.7 * parameters.dissonance;
        let refined = 0.3 + 0.7 * parameters.refinement;
        Self {
            parallels: 7.0 * strict,
            crossing: 4.0 * strict,
            spacing: 1.2 * refined,
            leaps: 1.4 * refined,
            // Not scaled, at any dissonance. The whole point of quoting a tune
            // is that it can be heard.
            melody_clash: 6.0,
            // Not scaled either: a note outside a line's range is not a liberty,
            // it is a note nobody can play.
            range: 5.0,
            doubling: 1.0 * strict,
        }
    }
}

/// The pitched lines of `bar`, low to high by the middle of their range.
///
/// Ordering by range rather than by pitch is what makes "crossing" mean
/// something: it is a voice sounding on the wrong side of the line it is
/// supposed to sit under, which is a fact about the arrangement and not about
/// one chord.
pub(crate) fn stacked(bar: &Bar) -> Vec<usize> {
    let mut order: Vec<usize> = (0..bar.voices.iter().filter(|v| v.is_pitched()).count()).collect();
    let pitched: Vec<&Voice> = bar.voices.iter().filter(|v| v.is_pitched()).collect();
    order.sort_by_key(|index| {
        pitched
            .get(*index)
            .map_or(0u16, |voice| u16::from(voice.low) + u16::from(voice.high))
    });
    order
}

/// Which pitched line is the lead, if any.
pub(crate) fn lead_index(bar: &Bar) -> Option<usize> {
    bar.voices
        .iter()
        .filter(|voice| voice.is_pitched())
        .position(|voice| voice.role == Role::Lead)
}

/// What `bar` costs against `previous`, under `weights`.
pub(crate) fn cost(bar: &Bar, previous: Option<&Bar>, weights: &Weights) -> f32 {
    let grid = Grid::of(bar);
    let pitched: Vec<&Voice> = bar.voices.iter().filter(|v| v.is_pitched()).collect();
    let order = stacked(bar);
    let lead = lead_index(bar);
    let join = previous.map(Grid::tail);

    let mut total = 0.0;
    total +=
        weights.parallels * num::of(crate::compose::analysis::parallel_perfects(bar, previous));
    total += weights.range * range_penalty(&pitched);
    total += weights.leaps * leap_penalty(&grid, join.as_deref(), lead);
    for row in &grid.rows {
        total += weights.crossing * crossing_penalty(row, &order);
        total += weights.spacing * spacing_penalty(row, &order);
        if let Some(lead) = lead {
            let (clash, doubling) = melody_penalty(row, lead);
            total += weights.melody_clash * clash + weights.doubling * doubling;
        }
    }
    total
}

/// How far outside their ranges the lines are written, in semitones halved.
fn range_penalty(pitched: &[&Voice]) -> f32 {
    let mut penalty = 0.0;
    for voice in pitched {
        for note in &voice.notes {
            let key = f32::from(note.key);
            penalty += (f32::from(voice.low) - key).max(0.0) / 2.0;
            penalty += (key - f32::from(voice.high)).max(0.0) / 2.0;
        }
    }
    penalty
}

/// How far the accompaniment moves, weighted by how far each move is.
fn leap_penalty(grid: &Grid, join: Option<&[Option<u8>]>, lead: Option<usize>) -> f32 {
    let width = grid.rows.first().map_or(0, Vec::len);
    let mut penalty = 0.0;
    for voice in 0..width {
        if Some(voice) == lead {
            // The tune is the tune. A leap in it is the composer of 1740's
            // decision, not this one's.
            continue;
        }
        let mut last = join.and_then(|join| join.get(voice).copied().flatten());
        for row in &grid.rows {
            let Some(key) = row.get(voice).copied().flatten() else {
                continue;
            };
            if let Some(before) = last {
                let interval = f32::from(before.abs_diff(key));
                penalty += if interval > 12.0 {
                    3.0
                } else if interval > 7.0 {
                    1.2
                } else if interval > 4.0 {
                    0.25
                } else {
                    0.0
                };
            }
            last = Some(key);
        }
    }
    penalty
}

/// How many pairs sound on the wrong side of each other.
fn crossing_penalty(row: &[Option<u8>], order: &[usize]) -> f32 {
    let mut penalty = 0.0;
    for pair in order.windows(2) {
        let [lower, upper] = pair else { continue };
        let (Some(low), Some(high)) = (
            row.get(*lower).copied().flatten(),
            row.get(*upper).copied().flatten(),
        ) else {
            continue;
        };
        if low > high {
            penalty += 1.0;
        }
    }
    penalty
}

/// How far the upper voices are spread past an octave.
fn spacing_penalty(row: &[Option<u8>], order: &[usize]) -> f32 {
    let mut penalty = 0.0;
    // The lowest pair is skipped: an octave and a half between the bass and
    // whatever is above it is normal writing, and closing it up is what makes
    // an arrangement sound like a keyboard exercise.
    for pair in order.windows(2).skip(1) {
        let [lower, upper] = pair else { continue };
        let (Some(low), Some(high)) = (
            row.get(*lower).copied().flatten(),
            row.get(*upper).copied().flatten(),
        ) else {
            continue;
        };
        let gap = f32::from(high.saturating_sub(low));
        if gap > 12.0 {
            penalty += (gap - 12.0) / 12.0;
        }
    }
    penalty
}

/// How much the accompaniment crowds the tune, and how much of it doubles the
/// tune outright.
fn melody_penalty(row: &[Option<u8>], lead: usize) -> (f32, f32) {
    let Some(tune) = row.get(lead).copied().flatten() else {
        return (0.0, 0.0);
    };
    let mut clash = 0.0;
    let mut doubling = 0.0;
    for (index, key) in row.iter().enumerate() {
        if index == lead {
            continue;
        }
        let Some(key) = *key else { continue };
        let distance = i16::from(tune) - i16::from(key);
        if distance < 0 {
            clash += 1.5;
        } else if distance == 0 {
            doubling += 0.4;
        } else if distance == 1 || distance == 11 {
            clash += 1.0;
        } else if distance == 2 || distance == 6 {
            clash += 0.5;
        }
    }
    (clash, doubling)
}
