//! Improving a voicing, and then making sure of it.
//!
//! Two passes, and they answer different questions. The anneal searches for a
//! *better* bar, over a space it cannot leave: an inner voice may take a
//! different chord tone in a different octave and nothing else, so whatever it
//! finds is still in the chord and the tune is untouched. The enforcement pass
//! then makes a *promise*: below
//! [`STRICT_DISSONANCE`](crate::Composer::STRICT_DISSONANCE) the bar it hands
//! back contains no parallel fifths or octaves at all, because a search that
//! usually succeeds is not something a test can rest on.

use alloc::vec::Vec;

use crate::compose::analysis::offences;
use crate::compose::cost::{Weights, cost, lead_index};
use crate::compose::{Bar, Role};
use crate::rng::Rng;

/// The temperature the anneal starts and ends at.
const HOT: f32 = 1.2;
/// The temperature the anneal ends at.
const COLD: f32 = 0.02;

/// The keys `voice` may take: chord tones inside its range and under the tune.
fn candidates(bar: &Bar, voice: usize, classes: &[u8], ceiling: u8) -> Vec<u8> {
    let Some(line) = bar.voices.get(voice) else {
        return Vec::new();
    };
    let high = line.high.min(ceiling);
    if line.low > high {
        return Vec::new();
    }
    (line.low..=high)
        .filter(|key| classes.contains(&(key % 12)))
        .collect()
}

/// Sets every note of `voice` to `key`.
fn hold_at(bar: &mut Bar, voice: usize, key: u8) {
    if let Some(line) = bar.voices.get_mut(voice) {
        for note in &mut line.notes {
            note.key = key;
        }
    }
}

/// Searches the inner voices for a cheaper bar.
///
/// Simulated annealing over a discrete space, seeded from the composer's own
/// generator so that the search is part of what a seed reproduces. The move is
/// always "this inner voice takes that chord tone instead", applied to the whole
/// line at once: an inner voice that changed pitch halfway through a bar would
/// be a second melody, and there is only one melody.
pub(crate) fn anneal(
    bar: &mut Bar,
    previous: Option<&Bar>,
    classes: &[u8],
    ceiling: u8,
    weights: &Weights,
    rng: &mut Rng,
    iterations: u32,
) {
    let movable: Vec<usize> = bar
        .voices
        .iter()
        .enumerate()
        .filter(|(_, voice)| voice.role == Role::Inner && !voice.notes.is_empty())
        .map(|(index, _)| index)
        .collect();
    if movable.is_empty() || iterations == 0 {
        return;
    }
    let mut current = cost(bar, previous, weights);
    let mut best = current;
    let mut best_keys: Vec<Option<u8>> = movable
        .iter()
        .map(|index| {
            bar.voices
                .get(*index)
                .and_then(|v| v.notes.first())
                .map(|n| n.key)
        })
        .collect();

    for step in 0..iterations {
        let Some(slot) = rng.below(movable.len()) else {
            break;
        };
        let Some(voice) = movable.get(slot).copied() else {
            break;
        };
        let options = candidates(bar, voice, classes, ceiling);
        let Some(choice) = rng
            .below(options.len())
            .and_then(|at| options.get(at).copied())
        else {
            continue;
        };
        let Some(before) = bar
            .voices
            .get(voice)
            .and_then(|line| line.notes.first())
            .map(|note| note.key)
        else {
            continue;
        };
        hold_at(bar, voice, choice);
        let candidate = cost(bar, previous, weights);
        let temperature = HOT
            * libm::powf(
                COLD / HOT,
                crate::num::of_u32(step) / crate::num::of_u32(iterations),
            );
        let delta = candidate - current;
        if delta <= 0.0 || rng.chance(libm::expf(-delta / temperature.max(1e-6))) {
            current = candidate;
            if candidate < best {
                best = candidate;
                best_keys = movable
                    .iter()
                    .map(|index| {
                        bar.voices
                            .get(*index)
                            .and_then(|v| v.notes.first())
                            .map(|n| n.key)
                    })
                    .collect();
            }
        } else {
            hold_at(bar, voice, before);
        }
    }
    for (slot, voice) in movable.iter().enumerate() {
        if let Some(Some(key)) = best_keys.get(slot).copied() {
            hold_at(bar, *voice, key);
        }
    }
}

/// How many times a single line may be worked on before it is silenced.
const ESCALATIONS: u8 = 4;

/// Removes every parallel fifth and octave from `bar`.
///
/// Four things are tried on the offending line, in order, and the last of them
/// always works. Another chord tone; holding it on one key for the whole bar,
/// which leaves it nothing to move in parallel *with*; delaying its entry, which
/// takes it out of the barline join; and finally silencing it, which is the same
/// answer the constructive pass gives to a voice with no room under the tune.
///
/// The tune is never the line that moves. Neither is a line already silent.
pub(crate) fn enforce(bar: &mut Bar, previous: Option<&Bar>, classes: &[u8], ceiling: u8) {
    let pitched: Vec<usize> = bar
        .voices
        .iter()
        .enumerate()
        .filter(|(_, voice)| voice.is_pitched())
        .map(|(index, _)| index)
        .collect();
    let lead = lead_index(bar);
    let mut escalation = alloc::vec![0u8; pitched.len()];

    let rounds =
        u32::try_from(pitched.len()).unwrap_or(0).saturating_add(1) * u32::from(ESCALATIONS);
    for _ in 0..rounds {
        let found = offences(bar, previous);
        let Some((first, second)) = found.first().copied() else {
            return;
        };
        // Prefer the upper line of the pair, and never the tune.
        let victim = [second, first]
            .into_iter()
            .find(|index| Some(*index) != lead)
            .unwrap_or(second);
        let Some(voice) = pitched.get(victim).copied() else {
            return;
        };
        let stage = escalation.get(victim).copied().unwrap_or(ESCALATIONS);
        if let Some(slot) = escalation.get_mut(victim) {
            *slot = slot.saturating_add(1);
        }
        match stage {
            0 => repitch(bar, previous, voice, victim, classes, ceiling),
            1 => {
                if let Some(key) = bar
                    .voices
                    .get(voice)
                    .and_then(|line| line.notes.first())
                    .map(|note| note.key)
                {
                    hold_at(bar, voice, key);
                }
            }
            2 => {
                if let Some(line) = bar.voices.get_mut(voice)
                    && !line.notes.is_empty()
                {
                    line.notes.remove(0);
                }
            }
            _ => {
                if let Some(line) = bar.voices.get_mut(voice) {
                    line.notes.clear();
                }
            }
        }
    }
}

/// Tries every chord tone available to `voice`, keeping the first that leaves it
/// in no parallel at all.
fn repitch(
    bar: &mut Bar,
    previous: Option<&Bar>,
    voice: usize,
    pitched_index: usize,
    classes: &[u8],
    ceiling: u8,
) {
    let Some(original) = bar
        .voices
        .get(voice)
        .and_then(|line| line.notes.first())
        .map(|note| note.key)
    else {
        return;
    };
    for key in candidates(bar, voice, classes, ceiling) {
        if key == original {
            continue;
        }
        hold_at(bar, voice, key);
        let clean = offences(bar, previous)
            .iter()
            .all(|(first, second)| *first != pitched_index && *second != pitched_index);
        if clean {
            return;
        }
    }
    hold_at(bar, voice, original);
}
