//! Merging a bank's layers into one set of numbers per sounding voice.
//!
//! A `SoundFont` describes a voice indirectly, and the merge rules are the
//! specification's: instrument-level generators are absolute, so a local zone
//! *replaces* the global one, and preset-level generators are relative, so they
//! are *added* on top of the resolved instrument value. Getting that backwards
//! is the classic way to make every bank sound almost right.

use alloc::vec::Vec;

use crate::num;
use crate::synth::bank::{Bank, GeneratorAmount, GeneratorKind, Preset, Sample, Zone};
use crate::synth::envelope::Shape;

/// One slot per generator the specification defines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Gens {
    values: [i32; GeneratorKind::COUNT as usize],
}

/// The generators whose default is not zero.
const DEFAULTS: [(GeneratorKind, i32); 7] = [
    (GeneratorKind::DELAY, -12_000),
    (GeneratorKind::ATTACK, -12_000),
    (GeneratorKind::HOLD, -12_000),
    (GeneratorKind::DECAY, -12_000),
    (GeneratorKind::RELEASE, -12_000),
    (GeneratorKind::SCALE_TUNING, 100),
    (GeneratorKind::ROOT_KEY, -1),
];

impl Gens {
    /// The specification's defaults.
    fn defaults() -> Self {
        let mut gens = Self {
            values: [0; GeneratorKind::COUNT as usize],
        };
        for (kind, value) in DEFAULTS {
            if let Some(slot) = gens.values.get_mut(usize::from(kind.0)) {
                *slot = value;
            }
        }
        gens
    }

    /// Reads one generator.
    fn get(&self, kind: GeneratorKind) -> i32 {
        self.values
            .get(usize::from(kind.0))
            .copied()
            .unwrap_or_default()
    }

    /// Reads one generator as the float its unit is measured in.
    fn getf(&self, kind: GeneratorKind) -> f32 {
        num::of_i32(self.get(kind))
    }

    /// Instrument-level merge: each generator replaces the slot.
    fn set_from(&mut self, zone: &Zone) {
        for generator in &zone.generators {
            if let Some(value) = scalar(generator.amount)
                && let Some(slot) = self.values.get_mut(usize::from(generator.kind.0))
            {
                *slot = value;
            }
        }
    }

    /// Preset-level merge: each generator is added to the slot.
    fn add_from(&mut self, zone: &Zone) {
        for generator in &zone.generators {
            if let Some(value) = scalar(generator.amount)
                && let Some(slot) = self.values.get_mut(usize::from(generator.kind.0))
            {
                *slot = slot.saturating_add(value);
            }
        }
    }
}

/// A generator's value as a number, or `None` for a range.
fn scalar(amount: GeneratorAmount) -> Option<i32> {
    match amount {
        GeneratorAmount::Signed(value) => Some(i32::from(value)),
        GeneratorAmount::Index(value) => Some(i32::from(value)),
        GeneratorAmount::Range { .. } => None,
    }
}

/// Everything one sounding layer of a note needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Articulation {
    /// Which sample to read.
    pub(crate) sample: usize,
    /// The group in which one note cuts another off; zero is no group.
    pub(crate) exclusive: i32,
    /// How far from the recorded pitch to play, in semitones.
    pub(crate) pitch: f32,
    /// Linear gain from the zone's attenuation.
    pub(crate) gain: f32,
    /// Stereo position, `-1.0` left to `1.0` right.
    pub(crate) pan: f32,
    /// The loop, in frames, when the zone says to loop.
    pub(crate) looping: Option<(u32, u32)>,
    /// The volume envelope.
    pub(crate) shape: Shape,
}

/// Timecents to seconds. A timecent of `-12000` or below is instantaneous.
fn seconds(timecents: f32) -> f32 {
    if timecents <= -12_000.0 {
        0.0
    } else {
        libm::exp2f(timecents / 1200.0)
    }
}

/// Centibels of attenuation to linear gain.
fn gain(centibels: f32) -> f32 {
    libm::powf(10.0, -centibels.clamp(0.0, 1440.0) / 200.0)
}

/// Splits a zone list into its optional leading global zone and the rest.
///
/// A first zone that names no `terminal` generator -- no instrument for a
/// preset, no sample for an instrument -- is global, and its generators are the
/// starting point for every zone after it.
fn split_global(zones: &[Zone], terminal: GeneratorKind) -> (Option<&Zone>, &[Zone]) {
    match zones.split_first() {
        Some((first, rest)) if first.index(terminal).is_none() => (Some(first), rest),
        _ => (None, zones),
    }
}

/// Every layer `preset` sounds for `key` at `velocity`.
///
/// A preset may have several zones matching one key, and an instrument several
/// matching one velocity, so one note can be two or five samples at once. That
/// is how a bank makes a note sound like an instrument rather than like a
/// recording, and it is why this answers a list.
pub(crate) fn resolve(bank: &Bank, preset: &Preset, key: u8, velocity: u8) -> Vec<Articulation> {
    let (preset_global, preset_zones) = split_global(&preset.zones, GeneratorKind::INSTRUMENT);
    let mut out = Vec::new();
    for zone in preset_zones {
        if !zone.matches(key, velocity) {
            continue;
        }
        let Some(instrument) = zone
            .index(GeneratorKind::INSTRUMENT)
            .and_then(|index| bank.instruments.get(usize::from(index)))
        else {
            continue;
        };
        let (instrument_global, instrument_zones) =
            split_global(&instrument.zones, GeneratorKind::SAMPLE_ID);
        for inner in instrument_zones {
            if !inner.matches(key, velocity) {
                continue;
            }
            let Some(index) = inner.index(GeneratorKind::SAMPLE_ID).map(usize::from) else {
                continue;
            };
            let Some(sample) = bank.samples.get(index) else {
                continue;
            };
            if !sample.kind.is_playable() || sample.pcm.is_empty() {
                continue;
            }
            let mut gens = Gens::defaults();
            if let Some(global) = instrument_global {
                gens.set_from(global);
            }
            gens.set_from(inner);
            if let Some(global) = preset_global {
                gens.add_from(global);
            }
            gens.add_from(zone);
            out.push(articulate(&gens, sample, index, key));
        }
    }
    out
}

/// Turns a merged generator set into an [`Articulation`].
fn articulate(gens: &Gens, sample: &Sample, index: usize, key: u8) -> Articulation {
    let root = gens.get(GeneratorKind::ROOT_KEY);
    let root_key = u8::try_from(root).unwrap_or(sample.original_key);

    // Keyboard tracking at `scale_tuning` cents a key, plus the fixed tuning
    // from the zone and the sample's own correction.
    let tracking = (f32::from(key) - f32::from(root_key)) * gens.getf(GeneratorKind::SCALE_TUNING);
    let tuning = gens.getf(GeneratorKind::COARSE_TUNE) * 100.0
        + gens.getf(GeneratorKind::FINE_TUNE)
        + f32::from(sample.correction);

    // Sample modes 1 and 3 loop; 0 and 2 play through once.
    let mode = gens.get(GeneratorKind::SAMPLE_MODES) & 0x3;
    let offset = |base: u32, fine: GeneratorKind, coarse: GeneratorKind| -> u32 {
        let value =
            i64::from(base) + i64::from(gens.get(fine)) + i64::from(gens.get(coarse)) * 32_768;
        u32::try_from(value.max(0)).unwrap_or(0)
    };
    let loop_start = offset(
        sample.loop_start,
        GeneratorKind::LOOP_START_OFFSET,
        GeneratorKind::LOOP_START_COARSE_OFFSET,
    );
    let loop_end = offset(
        sample.loop_end,
        GeneratorKind::LOOP_END_OFFSET,
        GeneratorKind::LOOP_END_COARSE_OFFSET,
    );

    Articulation {
        sample: index,
        exclusive: gens.get(GeneratorKind::EXCLUSIVE_CLASS),
        pitch: (tracking + tuning) / 100.0,
        gain: gain(gens.getf(GeneratorKind::ATTENUATION)),
        pan: (gens.getf(GeneratorKind::PAN) / 500.0).clamp(-1.0, 1.0),
        looping: ((mode == 1 || mode == 3) && loop_end > loop_start)
            .then_some((loop_start, loop_end)),
        shape: Shape {
            delay: seconds(gens.getf(GeneratorKind::DELAY)),
            attack: seconds(gens.getf(GeneratorKind::ATTACK)),
            hold: seconds(gens.getf(GeneratorKind::HOLD)),
            decay: seconds(gens.getf(GeneratorKind::DECAY)),
            sustain: gain(gens.getf(GeneratorKind::SUSTAIN)),
            release: seconds(gens.getf(GeneratorKind::RELEASE)),
        },
    }
}
