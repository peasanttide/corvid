//! Reading a bar back: what a rule says about it, and how like another one it
//! is.
//!
//! Everything here is a pure function of bars that already exist. The composer
//! uses it to score what it is about to write; a test uses the same functions to
//! say what it wrote is correct, which is deliberate -- a checker the composer
//! does not share is a second opinion about what the rule meant.

use alloc::vec::Vec;

use crate::compose::bar::EPSILON;
use crate::compose::{Bar, Note, Voice};
use crate::num;

/// The pitches sounding together, sampled at every onset any line has.
///
/// Counterpoint is a claim about what moves against what, and two lines with
/// different rhythms only line up at the moments one of them starts a note. So
/// the grid's columns are exactly those moments, and a cell is the key that
/// line is sounding then, or `None` when it is silent.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Grid {
    /// The beat each column samples.
    pub(crate) beats: Vec<f32>,
    /// One row per column, one cell per pitched line, in the order they appear
    /// in the bar.
    pub(crate) rows: Vec<Vec<Option<u8>>>,
}

impl Grid {
    /// Samples the pitched lines of `bar` at every onset in it.
    pub(crate) fn of(bar: &Bar) -> Self {
        let pitched: Vec<&Voice> = bar
            .voices
            .iter()
            .filter(|voice| voice.is_pitched())
            .collect();
        let mut beats: Vec<f32> = Vec::new();
        for voice in &pitched {
            for note in &voice.notes {
                if note.beat < bar.beats - EPSILON
                    && !beats.iter().any(|held| (held - note.beat).abs() < EPSILON)
                {
                    beats.push(note.beat);
                }
            }
        }
        beats.sort_by(|left, right| {
            left.partial_cmp(right)
                .unwrap_or(core::cmp::Ordering::Equal)
        });
        let rows = beats
            .iter()
            .map(|beat| pitched.iter().map(|voice| voice.sounding(*beat)).collect())
            .collect();
        Self { beats, rows }
    }

    /// The last key each pitched line of `bar` sounds, for joining across a
    /// barline.
    pub(crate) fn tail(bar: &Bar) -> Vec<Option<u8>> {
        bar.voices
            .iter()
            .filter(|voice| voice.is_pitched())
            .map(Voice::last_key)
            .collect()
    }
}

/// Whether two keys are a perfect fifth or a unison/octave apart.
fn is_perfect(lower: u8, upper: u8) -> bool {
    let interval = i16::from(upper).abs_diff(i16::from(lower)) % 12;
    interval == 0 || interval == 7
}

/// Collects the pairs of lines that move in parallel perfects between two
/// columns, as indices into the pitched lines.
fn parallels_between(before: &[Option<u8>], after: &[Option<u8>], out: &mut Vec<(usize, usize)>) {
    for (first, (before_first, after_first)) in before.iter().zip(after.iter()).enumerate() {
        for (second, (before_second, after_second)) in
            before.iter().zip(after.iter()).enumerate().skip(first + 1)
        {
            let (Some(a0), Some(a1), Some(b0), Some(b1)) =
                (*before_first, *after_first, *before_second, *after_second)
            else {
                continue;
            };
            // Neither voice moving is a held chord, not a parallel; one voice
            // moving cannot be parallel to anything.
            if a0 == a1 || b0 == b1 {
                continue;
            }
            let rising = (i16::from(a1) - i16::from(a0)).signum();
            let other = (i16::from(b1) - i16::from(b0)).signum();
            if rising == other && is_perfect(a0, b0) && is_perfect(a1, b1) {
                out.push((first, second));
            }
        }
    }
}

/// Every pair of pitched lines that moves in parallel perfects, as indices into
/// the pitched lines of `bar` in the order they appear.
///
/// A pair is listed once per offending moment, so a line that offends twice
/// appears twice: what the repair pass needs is which line to move and how
/// often, not a set.
pub(crate) fn offences(bar: &Bar, previous: Option<&Bar>) -> Vec<(usize, usize)> {
    let grid = Grid::of(bar);
    let mut found = Vec::new();
    if let Some(previous) = previous
        && let Some(first) = grid.rows.first()
    {
        parallels_between(&Grid::tail(previous), first, &mut found);
    }
    for pair in grid.rows.windows(2) {
        if let [before, after] = pair {
            parallels_between(before, after, &mut found);
        }
    }
    found
}

/// How many parallel fifths and octaves `bar` contains.
///
/// Counted between every pair of pitched lines at every point either of them
/// moves, and across the barline from `previous` when one is given -- a parallel
/// that happens on the join sounds exactly like one that happens inside a bar,
/// and a composer that only checked inside would write one at every barline.
///
/// Both voices must move, in the same direction, from one perfect interval to
/// another. A held note under a moving one is not a parallel and neither is
/// contrary motion between two fifths.
///
/// ```
/// use corvid_music::{Bar, Note, Role, Voice, parallel_perfects};
/// use corvid_music::{Chord, Mode, Parameters};
///
/// // Two lines a fifth apart, both stepping up a tone: the textbook offence.
/// let mut lower = Voice::new(Role::Bass, 36, 60);
/// lower.notes = vec![Note::new(48, 0.0, 1.0), Note::new(50, 1.0, 1.0)];
/// let mut upper = Voice::new(Role::Inner, 48, 72);
/// upper.notes = vec![Note::new(55, 0.0, 1.0), Note::new(57, 1.0, 1.0)];
///
/// let bar = Bar {
///     index: 0,
///     tempo: 96.0,
///     beats: 2.0,
///     beats_per_bar: 2.0,
///     tonic: 0,
///     mode: Mode::Ionian,
///     chord: Chord::on(0, Mode::Ionian, false),
///     cadence: None,
///     motif: None,
///     variation: 0,
///     elided: false,
///     voices: vec![lower, upper],
/// };
/// assert_eq!(parallel_perfects(&bar, None), 1);
/// ```
#[must_use]
pub fn parallel_perfects(bar: &Bar, previous: Option<&Bar>) -> usize {
    offences(bar, previous).len()
}

/// How alike two lines are in shape, from `0.0` to `1.0`.
///
/// Shape, not pitch: what is compared is the sequence of directions from one
/// note to the next, so a line and the same line transposed, or in another
/// mode, or an octave up, score `1.0`. That is the sense in which a motif can be
/// *recognised* after it has been transformed, and it is what
/// `tests/motif.rs` measures a recurrence with.
///
/// A line shorter than the other is compared over its own length and divided by
/// the longer one, so quoting the first half of a tune scores about a half
/// rather than a full mark. Either line having fewer than two notes scores
/// `0.0`, because a single note has no shape to compare.
///
/// ```
/// use corvid_music::{Note, contour_similarity};
///
/// let rising = [Note::new(60, 0.0, 1.0), Note::new(62, 1.0, 1.0), Note::new(64, 2.0, 1.0)];
/// let higher = [Note::new(72, 0.0, 1.0), Note::new(75, 1.0, 1.0), Note::new(79, 2.0, 1.0)];
/// let falling = [Note::new(64, 0.0, 1.0), Note::new(62, 1.0, 1.0), Note::new(60, 2.0, 1.0)];
///
/// assert_eq!(contour_similarity(&rising, &higher), 1.0);
/// assert_eq!(contour_similarity(&rising, &falling), 0.0);
/// ```
#[must_use]
pub fn contour_similarity(left: &[Note], right: &[Note]) -> f32 {
    let directions = |notes: &[Note]| -> Vec<i8> {
        notes
            .windows(2)
            .filter_map(|pair| match pair {
                [before, after] => Some(
                    i16::from(after.key)
                        .saturating_sub(i16::from(before.key))
                        .signum(),
                ),
                _ => None,
            })
            .filter_map(|step| i8::try_from(step).ok())
            .collect()
    };
    let left = directions(left);
    let right = directions(right);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let agreed = left
        .iter()
        .zip(right.iter())
        .filter(|(one, other)| one == other)
        .count();
    num::of(agreed) / num::of(left.len().max(right.len()))
}
