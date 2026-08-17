//! The material the composer quotes, and the memory that brings it back.

use alloc::vec::Vec;

use crate::compose::Step;
use crate::rng::Rng;

/// Which motif this is.
///
/// A number a data pack answers to. Nothing here resolves one into a source, a
/// citation or a date; a pool holds what a pack put in it and the pack decides
/// what may be in it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(::serde::Serialize, ::serde::Deserialize),
    serde(transparent)
)]
pub struct MotifId(
    /// The pack's number for it.
    pub u32,
);

/// What a motif is *about*.
///
/// Deliberately opaque: this crate does not know whether a subject is a person,
/// a place or an event, only that a caller can say one is present and that
/// saying so makes the motifs bound to it more likely to be drawn. That is the
/// whole of the association, and it is what lets a theme come back when its
/// subject does without this crate learning a single thing about the game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(::serde::Serialize, ::serde::Deserialize),
    serde(transparent)
)]
pub struct Subject(
    /// The caller's number for it.
    pub u32,
);

/// One note of a motif: a place in the scale, or a rest, and how long it lasts.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Event {
    /// Where in the scale, or `None` for a rest.
    pub step: Option<Step>,
    /// How long, in beats.
    pub beats: f32,
}

impl Event {
    /// A note on `step` lasting `beats`.
    #[must_use]
    pub const fn note(step: Step, beats: f32) -> Self {
        Self {
            step: Some(step),
            beats,
        }
    }

    /// A rest lasting `beats`.
    #[must_use]
    pub const fn rest(beats: f32) -> Self {
        Self { step: None, beats }
    }
}

/// A short idea in degree space, with what it is about and how present that is.
///
/// The events are the tune and are never edited: every variation is a
/// [`Transform`] applied to a copy, so the thing a listener recognises stays
/// exactly what a pack recorded.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Motif {
    /// Which motif this is.
    pub id: MotifId,
    /// What it is about, or nothing.
    pub subject: Option<Subject>,
    /// The tune, as written.
    pub events: Vec<Event>,
    /// How present its subject has been lately. Raised by
    /// [`MotifPool::warm`], lowered by [`MotifPool::cool`], and the weight a
    /// draw is made against.
    pub heat: f32,
}

impl Motif {
    /// A motif about nothing, at rest.
    #[must_use]
    pub const fn new(id: MotifId, events: Vec<Event>) -> Self {
        Self {
            id,
            subject: None,
            events,
            heat: 0.0,
        }
    }

    /// Binds it to `subject`.
    #[must_use]
    pub const fn about(mut self, subject: Subject) -> Self {
        self.subject = Some(subject);
        self
    }

    /// Sets its starting heat.
    #[must_use]
    pub const fn with_heat(mut self, heat: f32) -> Self {
        self.heat = heat;
        self
    }

    /// How long the motif lasts, in beats.
    #[must_use]
    pub fn beats(&self) -> f32 {
        self.events.iter().map(|event| event.beats.max(0.0)).sum()
    }
}

/// A transformation of a motif, in degree space.
///
/// Every one of them is diatonic by construction, because it moves degrees
/// rather than semitones, so a transformed motif is still in the mode and its
/// alterations are still the notes that were altered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub enum Transform {
    /// Up or down by scale steps.
    Transpose(i8),
    /// Turned upside down about its first sounding note, alterations included.
    Invert,
    /// Played backwards.
    Retrograde,
    /// Twice as slow.
    Augment,
    /// Twice as fast.
    Diminish,
}

/// Applies `chain` to `events`, left to right.
///
/// ```
/// use corvid_music::{Event, Step, Transform, transform};
///
/// let phrase = [Event::note(Step::new(0), 1.0), Event::note(Step::new(2), 1.0)];
/// let up = transform(&phrase, &[Transform::Transpose(1)]);
/// assert_eq!(up[0].step.map(|s| s.degree), Some(1));
/// assert_eq!(up[1].step.map(|s| s.degree), Some(3));
///
/// let back = transform(&phrase, &[Transform::Retrograde]);
/// assert_eq!(back[0].step.map(|s| s.degree), Some(2));
/// ```
#[must_use]
pub fn transform(events: &[Event], chain: &[Transform]) -> Vec<Event> {
    let mut current: Vec<Event> = events.to_vec();
    for step in chain {
        current = apply(&current, *step);
    }
    current
}

/// One transformation.
fn apply(events: &[Event], transform: Transform) -> Vec<Event> {
    match transform {
        Transform::Transpose(steps) => events
            .iter()
            .map(|event| Event {
                step: event.step.map(|step| Step {
                    degree: step.degree.saturating_add(steps),
                    ..step
                }),
                ..*event
            })
            .collect(),
        Transform::Invert => invert(events),
        Transform::Retrograde => events.iter().rev().copied().collect(),
        Transform::Augment => scale_time(events, 2.0),
        Transform::Diminish => scale_time(events, 0.5),
    }
}

/// Turns a line upside down about its first sounding note.
fn invert(events: &[Event]) -> Vec<Event> {
    let Some(axis) = events
        .iter()
        .find_map(|event| event.step)
        .map(|step| i16::from(step.degree) + i16::from(step.octave) * 7)
    else {
        return events.to_vec();
    };
    events
        .iter()
        .map(|event| Event {
            step: event.step.map(|step| {
                let absolute = i16::from(step.degree) + i16::from(step.octave) * 7;
                let mirrored = 2 * axis - absolute;
                Step {
                    degree: i8::try_from(mirrored.rem_euclid(7)).unwrap_or(0),
                    octave: i8::try_from(mirrored.div_euclid(7)).unwrap_or(0),
                    alteration: step.alteration.saturating_neg(),
                }
            }),
            ..*event
        })
        .collect()
}

/// Stretches every duration by `factor`.
fn scale_time(events: &[Event], factor: f32) -> Vec<Event> {
    events
        .iter()
        .map(|event| Event {
            beats: event.beats * factor,
            ..*event
        })
        .collect()
}

/// The motifs a composer may quote, and how present each one's subject is.
///
/// A pool is drawn from by heat, so the tune that played when a subject was
/// last present comes back when it is present again -- transformed by whatever
/// the parameters are by then, but the same tune. That costs one float per
/// motif and is the entire mechanism.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct MotifPool {
    motifs: Vec<Motif>,
}

/// The heat a motif is drawn at when its subject has never been named, so that
/// a cold pool still yields something rather than nothing.
const FLOOR_HEAT: f32 = 0.05;

impl MotifPool {
    /// An empty pool.
    #[must_use]
    pub const fn new() -> Self {
        Self { motifs: Vec::new() }
    }

    /// Adds `motif`, replacing any motif already under its identifier.
    pub fn insert(&mut self, motif: Motif) {
        match self.motifs.iter_mut().find(|held| held.id == motif.id) {
            Some(held) => *held = motif,
            None => self.motifs.push(motif),
        }
    }

    /// The motif under `id`.
    #[must_use]
    pub fn get(&self, id: MotifId) -> Option<&Motif> {
        self.motifs.iter().find(|motif| motif.id == id)
    }

    /// How many motifs are in the pool.
    #[must_use]
    pub fn len(&self) -> usize {
        self.motifs.len()
    }

    /// Whether the pool is empty, in which case a composer has nothing to quote
    /// and every bar it writes is accompaniment.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.motifs.is_empty()
    }

    /// Every motif, in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &Motif> {
        self.motifs.iter()
    }

    /// Raises the heat of every motif bound to `subject` by `amount`.
    ///
    /// Called when the subject is present. Heat is not capped here: a caller
    /// that warms every bar decides the ceiling by how hard it
    /// [`cool`](Self::cool)s.
    pub fn warm(&mut self, subject: Subject, amount: f32) {
        for motif in &mut self.motifs {
            if motif.subject == Some(subject) {
                motif.heat += amount;
            }
        }
    }

    /// Decays every motif's heat by `factor`, which is clamped into
    /// `0.0 ..= 1.0`.
    ///
    /// A composer calls this once a bar, so a subject that stops being present
    /// fades out of the music over a phrase rather than at the next barline.
    pub fn cool(&mut self, factor: f32) {
        let factor = factor.clamp(0.0, 1.0);
        for motif in &mut self.motifs {
            motif.heat *= factor;
        }
    }

    /// Draws a motif, weighted by heat, avoiding `avoid` where it can.
    ///
    /// The weight is `heat` plus a floor, so a pool nobody has warmed still
    /// answers and a pool somebody has warmed hard answers with what they
    /// warmed. `avoid` is a courtesy rather than a rule: when it covers
    /// everything, the draw is made over everything, because a composer with
    /// something to play beats a composer being tasteful about repetition.
    pub(crate) fn draw(&self, rng: &mut Rng, avoid: &[MotifId]) -> Option<MotifId> {
        let weights: Vec<f32> = self
            .motifs
            .iter()
            .map(|motif| {
                if avoid.contains(&motif.id) {
                    0.0
                } else {
                    motif.heat.max(0.0) + FLOOR_HEAT
                }
            })
            .collect();
        let drawn = rng.weighted(&weights).or_else(|| {
            let all: Vec<f32> = self
                .motifs
                .iter()
                .map(|motif| motif.heat.max(0.0) + FLOOR_HEAT)
                .collect();
            rng.weighted(&all)
        })?;
        self.motifs.get(drawn).map(|motif| motif.id)
    }
}
