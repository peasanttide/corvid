//! The span a form lasts, and the chord each of its bars is built on.
//!
//! A phrase is what the parameters that need a boundary are allowed to change
//! at: the mode, the metre, how many lines there are, which motif is being
//! quoted. The parameters that can act inside a bar -- density, ornament,
//! register, grit -- never wait for one.

use alloc::vec::Vec;

use crate::compose::melody::Quote;
use crate::compose::{Cadence, Chord, Event, Mode, Motif, MotifId, MotifPool, Note, Parameters};
use crate::compose::{Transform, transform};
use crate::rng::Rng;

/// One span of the music, and where it has got to.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Phrase {
    /// The bar this phrase started on.
    pub(crate) start: u32,
    /// How many bars long it is.
    pub(crate) length: u32,
    /// The mode every degree in it resolves through.
    pub(crate) mode: Mode,
    /// The pitch class of its tonic.
    pub(crate) tonic: u8,
    /// Its metre, in beats.
    pub(crate) beats_per_bar: f32,
    /// How many pitched lines it was laid out for.
    pub(crate) voices: u8,
    /// The motif being quoted, if any.
    pub(crate) motif: Option<MotifId>,
    /// The motif after this variation's transformations.
    pub(crate) events: Vec<Event>,
    /// Which variation: `0` is the tune as the pack wrote it.
    pub(crate) variation: u32,
    /// Where in the tune the next bar starts.
    pub(crate) quote: Quote,
}

/// The metres a phrase may be in, and how each is weighted.
///
/// Two is the dance, three is the court, four is the march. Refinement decides
/// between them, which is why the same tune is a bourree in one street and a
/// menuet in another.
fn metre(parameters: Parameters, rng: &mut Rng) -> f32 {
    let weights = [
        1.2 * (1.0 - parameters.refinement) + 0.3,
        0.2 + 1.1 * parameters.refinement,
        0.8,
    ];
    match rng.weighted(&weights) {
        Some(1) => 3.0,
        Some(2) => 4.0,
        _ => 2.0,
    }
}

/// The transformations variation `index` applies to a motif.
///
/// Applied once, deliberately, when the variation begins -- never by random
/// search. The tune a listener hears is the tune, transformed on purpose.
fn chain(index: u32, rng: &mut Rng) -> Vec<Transform> {
    if index == 0 {
        return Vec::new();
    }
    let menu: [&[Transform]; 7] = [
        &[Transform::Transpose(2)],
        &[Transform::Transpose(-2)],
        &[Transform::Invert],
        &[Transform::Retrograde],
        &[Transform::Augment],
        &[Transform::Diminish],
        &[Transform::Invert, Transform::Transpose(1)],
    ];
    rng.below(menu.len())
        .and_then(|at| menu.get(at))
        .map_or_else(Vec::new, |steps| steps.to_vec())
}

/// Where a new phrase's tune comes from.
///
/// Three things that are one decision -- which motifs there are, which of them
/// have just been heard, and which one a trigger insists on -- travelling
/// together because nothing ever wants one of them without the others.
pub(crate) struct Material<'a> {
    /// Everything that may be quoted.
    pub(crate) pool: &'a MotifPool,
    /// What has just been quoted, and so is held back from this draw.
    pub(crate) recent: &'a [MotifId],
    /// What a trigger insists on, overriding the draw.
    pub(crate) forced: Option<MotifId>,
}

impl Phrase {
    /// Begins a phrase at bar `start`.
    ///
    /// The tonic follows the phrase before it unless refinement is high enough
    /// to want a modulation, and then it moves by a fifth -- the only distance a
    /// listener hears as the same music somewhere else rather than as a mistake.
    pub(crate) fn begin(
        previous: Option<&Self>,
        parameters: Parameters,
        mode: Mode,
        start: u32,
        material: &Material<'_>,
        rng: &mut Rng,
    ) -> Self {
        let tonic = previous.map_or(0, |phrase| {
            if parameters.refinement > 0.7 && rng.chance(0.25) {
                (phrase.tonic + 7) % 12
            } else {
                phrase.tonic
            }
        });
        let motif = material
            .forced
            .or_else(|| material.pool.draw(rng, material.recent));
        let events = motif
            .and_then(|id| material.pool.get(id))
            .map(|held| held.events.clone())
            .unwrap_or_default();
        Self {
            start,
            length: if parameters.harmonic_rate > 0.6 { 4 } else { 8 },
            mode,
            tonic,
            beats_per_bar: metre(parameters, rng),
            voices: parameters.voices,
            motif,
            events,
            variation: 0,
            quote: Quote::default(),
        }
    }

    /// Applies the next variation's transformations, in place.
    ///
    /// Called when the tune has been round once, so a phrase longer than its
    /// motif is a set of variations rather than a loop.
    pub(crate) fn vary(&mut self, source: &Motif, rng: &mut Rng) {
        self.variation = self.variation.saturating_add(1);
        self.events = transform(&source.events, &chain(self.variation, rng));
        self.quote = Quote::default();
    }
}

/// Which scale degree `key` is, or `None` when it is outside the mode.
pub(crate) fn degree_of(key: u8, tonic: u8, mode: Mode) -> Option<u8> {
    let semitone = i8::try_from((i16::from(key) - i16::from(tonic % 12)).rem_euclid(12)).ok()?;
    mode.semitones()
        .iter()
        .position(|held| *held == semitone)
        .and_then(|index| u8::try_from(index).ok())
}

/// How much weight a moment in the bar carries.
fn strength(beat: f32, beats_per_bar: f32) -> f32 {
    if beat.abs() < 1e-4 {
        3.0
    } else if (beat - beats_per_bar / 2.0).abs() < 1e-4 {
        2.0
    } else if (beat - libm::roundf(beat)).abs() < 1e-4 {
        1.5
    } else {
        0.6
    }
}

/// How well `chord` supports `notes`.
///
/// The direction is deliberate and is the one rule the search may never
/// reverse: the melody is quoted material, so the chord is chosen to fit the
/// melody and only the accompaniment is searched. A tune annealed into notes
/// that are in key, on chord tones and no longer the tune is what this prevents.
fn fit(chord: Chord, notes: &[Note], setting: &Setting, chromatic: f32) -> f32 {
    let mut score = 0.0;
    for note in notes {
        let weight = strength(note.beat, setting.beats_per_bar) * note.beats.clamp(0.25, 1.25);
        match degree_of(note.key, setting.tonic, setting.mode) {
            Some(degree) if chord.contains(i8::try_from(degree).unwrap_or(0)) => score += weight,
            // A note outside the mode wants a chord that owns it, and this
            // crate does not build one -- so it is a passing note, and how much
            // that costs is what chromaticism buys down.
            None => score -= weight * 0.8 * (1.0 - chromatic),
            Some(_) => score -= weight * 0.8,
        }
    }
    score
}

/// What the chord for one bar is chosen against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Setting {
    /// The chord the bar before was built on.
    pub(crate) previous: Option<Chord>,
    /// The cadence this bar must be written on, when it must be written on one.
    pub(crate) forced: Option<Cadence>,
    /// The pitch class every degree resolves from.
    pub(crate) tonic: u8,
    /// The mode every degree resolves through.
    pub(crate) mode: Mode,
    /// The metre, which decides which beats carry weight.
    pub(crate) beats_per_bar: f32,
}

/// Chooses the chord this bar is built on.
///
/// Melody fit first, then functional pull from the chord before it, then the
/// cadence the phrase is heading for. `forced` overrides all three, which is
/// how a cadence -- landed or deferred -- gets the chord it needs.
pub(crate) fn choose(
    notes: &[Note],
    setting: &Setting,
    parameters: Parameters,
    rng: &mut Rng,
) -> Chord {
    let Setting {
        previous,
        forced,
        mode,
        ..
    } = *setting;
    if let Some(cadence) = forced {
        let degree = match cadence {
            Cadence::Authentic | Cadence::Plagal => 0,
            Cadence::Half => 4,
            Cadence::Deceptive => 5,
        };
        return Chord::on(degree, mode, false);
    }
    let seventh_chance = 0.15 + 0.5 * parameters.chromaticism;
    let mut best: Option<(f32, Chord)> = None;
    for degree in 0u8..7 {
        for seventh in [false, true] {
            if seventh && !(degree == 4 || parameters.refinement > 0.7) {
                continue;
            }
            let chord = Chord::on(degree, mode, seventh);
            let mut score = fit(chord, notes, setting, parameters.chromaticism) * 2.0;
            if let Some(previous) = previous {
                if pulls(previous.degree, degree) {
                    score += 1.4;
                }
                if degree == previous.degree {
                    // Sitting still is not a progression, and how much the
                    // harmony wants to move is what harmonic rate means.
                    score -= 0.6 + 2.4 * parameters.harmonic_rate;
                }
            }
            if seventh {
                score += seventh_chance * 2.0 - 1.0;
            }
            if parameters.refinement < 0.35 && !matches!(degree, 0 | 3 | 4 | 5) {
                // The pub end of the ladder really does only know three chords.
                score -= 2.5;
            }
            score += rng.unit() * 0.5;
            if best.is_none_or(|(held, _)| score > held) {
                best = Some((score, chord));
            }
        }
    }
    best.map_or_else(|| Chord::on(0, mode, false), |(_, chord)| chord)
}

/// Whether the harmony ordinarily moves from `from` to `to`.
fn pulls(from: u8, to: u8) -> bool {
    let table: [&[u8]; 7] = [
        &[3, 4, 5, 1],
        &[4, 6],
        &[5, 3],
        &[4, 0, 1],
        &[0, 5],
        &[3, 1, 4],
        &[0],
    ];
    table
        .get(usize::from(from % 7))
        .is_some_and(|targets| targets.contains(&to))
}
