//! What a `SoundFont` bank is, once the container has been thrown away.
//!
//! The model is the specification's own: a preset is what a bank-select and a
//! program-change address, a preset zone points at an instrument, an instrument
//! zone points at a sample, and every zone carries generators that say how the
//! sample is articulated. Nothing here remembers that it came out of a RIFF
//! file.

use alloc::string::String;
use alloc::vec::Vec;

/// A synthesis parameter a zone sets.
///
/// A transparent number rather than an enumeration, and deliberately: the
/// specification defines sixty generators, this crate acts on about half of
/// them, and a bank in the wild carries whichever it likes. An enumeration would
/// have to drop what it did not recognise, which is how a bank quietly loses the
/// one generator that made an instrument sound right. The constants below name
/// the ones that are acted on; everything else is carried through as the number
/// it was.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(::serde::Serialize, ::serde::Deserialize),
    serde(transparent)
)]
pub struct GeneratorKind(
    /// The specification's operator number.
    pub u8,
);

impl GeneratorKind {
    /// How many generators the specification defines.
    pub const COUNT: u8 = 60;

    /// Sample points added to the sample's start.
    pub const START_OFFSET: Self = Self(0);
    /// Sample points added to the sample's end.
    pub const END_OFFSET: Self = Self(1);
    /// Sample points added to the loop's start.
    pub const LOOP_START_OFFSET: Self = Self(2);
    /// Sample points added to the loop's end.
    pub const LOOP_END_OFFSET: Self = Self(3);
    /// Thirty-two-thousand-sample-point units added to the sample's start.
    pub const START_COARSE_OFFSET: Self = Self(4);
    /// Thirty-two-thousand-sample-point units added to the sample's end.
    pub const END_COARSE_OFFSET: Self = Self(12);
    /// Stereo position, in tenths of a percent, negative to the left.
    pub const PAN: Self = Self(17);
    /// Volume envelope delay, in timecents.
    pub const DELAY: Self = Self(33);
    /// Volume envelope attack, in timecents.
    pub const ATTACK: Self = Self(34);
    /// Volume envelope hold, in timecents.
    pub const HOLD: Self = Self(35);
    /// Volume envelope decay, in timecents.
    pub const DECAY: Self = Self(36);
    /// Volume envelope sustain, in centibels below peak.
    pub const SUSTAIN: Self = Self(37);
    /// Volume envelope release, in timecents.
    pub const RELEASE: Self = Self(38);
    /// Which instrument a preset zone plays.
    pub const INSTRUMENT: Self = Self(41);
    /// The keys this zone answers to.
    pub const KEY_RANGE: Self = Self(43);
    /// The velocities this zone answers to.
    pub const VELOCITY_RANGE: Self = Self(44);
    /// Thirty-two-thousand-sample-point units added to the loop's start.
    pub const LOOP_START_COARSE_OFFSET: Self = Self(45);
    /// Attenuation, in centibels.
    pub const ATTENUATION: Self = Self(48);
    /// Thirty-two-thousand-sample-point units added to the loop's end.
    pub const LOOP_END_COARSE_OFFSET: Self = Self(50);
    /// Tuning, in semitones.
    pub const COARSE_TUNE: Self = Self(51);
    /// Tuning, in cents.
    pub const FINE_TUNE: Self = Self(52);
    /// Which sample an instrument zone plays.
    pub const SAMPLE_ID: Self = Self(53);
    /// Whether and how the sample loops.
    pub const SAMPLE_MODES: Self = Self(54);
    /// Cents per key of keyboard tracking; `100` is equal temperament.
    pub const SCALE_TUNING: Self = Self(56);
    /// The group within which one note cuts another off.
    pub const EXCLUSIVE_CLASS: Self = Self(57);
    /// The key the sample was recorded at, overriding the sample's own.
    pub const ROOT_KEY: Self = Self(58);
}

/// The value of a generator.
///
/// The two bytes in the file are a union, and which reading is right depends on
/// the generator. This is that decision, made once at parse time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub enum GeneratorAmount {
    /// A signed amount: the common case.
    Signed(i16),
    /// An unsigned index, for `INSTRUMENT` and `SAMPLE_ID`.
    Index(u16),
    /// An inclusive range, for the two range generators.
    Range {
        /// Lowest value that matches.
        low: u8,
        /// Highest value that matches.
        high: u8,
    },
}

/// One synthesis parameter and its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Generator {
    /// Which parameter.
    pub kind: GeneratorKind,
    /// Its value, read according to `kind`.
    pub amount: GeneratorAmount,
}

/// A region of the key and velocity space, and how it is articulated.
///
/// The first zone of a preset or an instrument is a *global* zone when it names
/// no instrument or sample: its generators apply to every zone after it.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Zone {
    /// The generators, in file order.
    pub generators: Vec<Generator>,
}

impl Zone {
    /// The value of `kind` in this zone, when it is a plain number.
    #[must_use]
    pub fn amount(&self, kind: GeneratorKind) -> Option<i32> {
        self.generators
            .iter()
            .find(|generator| generator.kind == kind)
            .and_then(|generator| match generator.amount {
                GeneratorAmount::Signed(value) => Some(i32::from(value)),
                GeneratorAmount::Index(value) => Some(i32::from(value)),
                GeneratorAmount::Range { .. } => None,
            })
    }

    /// The index `kind` names, when it names one.
    #[must_use]
    pub fn index(&self, kind: GeneratorKind) -> Option<u16> {
        self.generators
            .iter()
            .find(|generator| generator.kind == kind)
            .and_then(|generator| match generator.amount {
                GeneratorAmount::Index(value) => Some(value),
                GeneratorAmount::Signed(value) => u16::try_from(value).ok(),
                GeneratorAmount::Range { .. } => None,
            })
    }

    /// The range `kind` names, when it names one.
    #[must_use]
    pub fn range(&self, kind: GeneratorKind) -> Option<(u8, u8)> {
        self.generators
            .iter()
            .find(|generator| generator.kind == kind)
            .and_then(|generator| match generator.amount {
                GeneratorAmount::Range { low, high } => Some((low, high)),
                _ => None,
            })
    }

    /// Whether this zone answers to `key` at `velocity`.
    ///
    /// A missing range generator matches everything on that axis, which is the
    /// specification's own default and is how a single-zone instrument covers
    /// the keyboard.
    #[must_use]
    pub fn matches(&self, key: u8, velocity: u8) -> bool {
        let inside = |range: Option<(u8, u8)>, value: u8| {
            range.is_none_or(|(low, high)| value >= low && value <= high)
        };
        inside(self.range(GeneratorKind::KEY_RANGE), key)
            && inside(self.range(GeneratorKind::VELOCITY_RANGE), velocity)
    }
}

/// What a bank-select and a program-change address.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Preset {
    /// Its name.
    pub name: String,
    /// Its program number.
    pub program: u16,
    /// Its bank number. `128` is the percussion bank by convention.
    pub bank: u16,
    /// Its zones, the first of which may be global.
    pub zones: Vec<Zone>,
}

/// A layer of sample zones one or more presets share.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Instrument {
    /// Its name.
    pub name: String,
    /// Its zones, the first of which may be global.
    pub zones: Vec<Zone>,
}

/// Where a sample sits in a stereo pair, if it does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub enum SampleKind {
    /// On its own.
    #[default]
    Mono,
    /// The left channel of a pair.
    Left,
    /// The right channel of a pair.
    Right,
    /// One channel of a linked set.
    Linked,
    /// In the wavetable ROM, and so not in the file at all.
    Rom,
    /// A flag this crate does not recognise, kept as it was found.
    Other(u16),
}

impl SampleKind {
    /// Whether this crate can play a sample of this kind.
    ///
    /// A ROM sample's audio is in hardware nobody has had since 1998, so a bank
    /// that names one is describing a sound this crate cannot make.
    #[must_use]
    pub const fn is_playable(self) -> bool {
        !matches!(self, Self::Rom)
    }
}

/// One channel of audio and the parameters that play it.
///
/// The audio is decoded 16-bit PCM and the loop points are relative to it, so
/// nothing here refers back to the file's global sample pool.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Sample {
    /// Its name.
    pub name: String,
    /// The audio.
    pub pcm: Vec<i16>,
    /// The rate it was recorded at.
    pub sample_rate: u32,
    /// Where the loop starts, in frames into `pcm`.
    pub loop_start: u32,
    /// Where the loop ends, in frames into `pcm`.
    pub loop_end: u32,
    /// The key it was recorded at.
    pub original_key: u8,
    /// Its tuning correction, in cents.
    pub correction: i8,
    /// Where it sits in a stereo pair.
    pub kind: SampleKind,
}

/// A whole bank: presets, the instruments they reach, and the samples those
/// play.
///
/// Built by [`Bank::parse`] from a `.sf2` image. There is no loader here and no
/// file: a bank arrives as bytes, from wherever a game keeps its packs.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Bank {
    /// The bank's name, when it gave one.
    pub name: Option<String>,
    /// The presets.
    pub presets: Vec<Preset>,
    /// The instruments preset zones reach.
    pub instruments: Vec<Instrument>,
    /// The samples instrument zones play.
    pub samples: Vec<Sample>,
}

impl Bank {
    /// The preset at `bank` and `program`, falling back the way a General MIDI
    /// player does: the same program in bank zero, then the first preset there
    /// is.
    ///
    /// A bank with no presets at all answers `None`, and a synthesizer holding
    /// one makes no sound rather than an arbitrary one.
    #[must_use]
    pub fn preset(&self, bank: u16, program: u16) -> Option<&Preset> {
        self.presets
            .iter()
            .find(|preset| preset.bank == bank && preset.program == program)
            .or_else(|| {
                self.presets
                    .iter()
                    .find(|preset| preset.bank == 0 && preset.program == program)
            })
            .or_else(|| self.presets.first())
    }
}
