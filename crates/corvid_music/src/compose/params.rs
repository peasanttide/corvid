//! The twelve dials the composer is driven by.

/// The twelve musical parameters a bar is composed from.
///
/// Every one but [`tempo`](Self::tempo) and [`voices`](Self::voices) is a
/// proportion in `0.0 ..= 1.0`. What sets them is not this crate's business:
/// they are musical quantities, and a game that maps its own state onto them
/// does so on its own side of the fence. What this crate promises is that the
/// same twelve numbers and the same seed give the same bar.
///
/// They are recomputed every bar. Six of them act inside the bar they arrive
/// in -- density, ornament, register, grit, syncopation and dissonance -- and
/// six need a boundary, because they choose the mode, the metre and how many
/// lines there are.
///
/// ```
/// use corvid_music::Parameters;
///
/// let calm = Parameters::default();
/// assert_eq!(calm.voices, 3);
///
/// // Out-of-range values are the caller's arithmetic, not an error: they clamp.
/// let wild = Parameters { density: 4.0, mode_dark: -1.0, ..Parameters::default() };
/// let clamped = wild.clamped();
/// assert_eq!(clamped.density, 1.0);
/// assert_eq!(clamped.mode_dark, 0.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Parameters {
    /// Beats per minute.
    pub tempo: f32,
    /// How many notes to a beat the accompaniment aims for.
    pub density: f32,
    /// How many pitched lines to write, `1 ..= 7`.
    pub voices: u8,
    /// How far the counterpoint rules may be broken. Every rule that is a
    /// matter of taste is scaled by `1 - 0.7 * dissonance`; two are never
    /// scaled at all.
    pub dissonance: f32,
    /// How often a note may leave the mode.
    pub chromaticism: f32,
    /// How often the lead is decorated.
    pub ornament: f32,
    /// Where on the ladder from lydian to phrygian the mode sits.
    pub mode_dark: f32,
    /// How much weight lands off the beat.
    pub syncopation: f32,
    /// How often the harmony is willing to move.
    pub harmonic_rate: f32,
    /// How much the leap, spacing and repetition rules are worth.
    pub refinement: f32,
    /// How high the lead sits in its range.
    pub register: f32,
    /// How much percussion, and how hard.
    pub grit: f32,
}

impl Parameters {
    /// A serviceable middle: a walking tempo, three voices, and every
    /// proportion at something a listener would call ordinary.
    ///
    /// Not [`Default::default`]'s zeroes. A zeroed `Parameters` is a silent,
    /// tempoless, voiceless bar, which is a trap rather than a starting point --
    /// the same reason [`corvid_sound`](https://docs.rs/corvid_sound)'s listener
    /// does not derive its gain.
    pub const DEFAULT: Self = Self {
        tempo: 96.0,
        density: 0.45,
        voices: 3,
        dissonance: 0.15,
        chromaticism: 0.1,
        ornament: 0.2,
        mode_dark: 0.3,
        syncopation: 0.2,
        harmonic_rate: 0.4,
        refinement: 0.6,
        register: 0.5,
        grit: 0.2,
    };

    /// The most voices a bar is written for.
    ///
    /// Seven: the point past which another inner line stops being audible as a
    /// line and starts being thickness, and the count the ranges below are laid
    /// out for.
    pub const MAX_VOICES: u8 = 7;

    /// The slowest and fastest tempo a bar is written at.
    pub const TEMPO_RANGE: (f32, f32) = (20.0, 300.0);

    /// The same parameters with every field brought into range.
    ///
    /// The composer calls this on everything it is given, so a caller never has
    /// to; it is public because a debug overlay wants to show what the composer
    /// actually used rather than what it was handed.
    #[must_use]
    pub fn clamped(self) -> Self {
        let unit = |value: f32| {
            if value.is_nan() {
                0.0
            } else {
                value.clamp(0.0, 1.0)
            }
        };
        let (slowest, fastest) = Self::TEMPO_RANGE;
        Self {
            tempo: if self.tempo.is_nan() {
                Self::DEFAULT.tempo
            } else {
                self.tempo.clamp(slowest, fastest)
            },
            density: unit(self.density),
            voices: self.voices.clamp(1, Self::MAX_VOICES),
            dissonance: unit(self.dissonance),
            chromaticism: unit(self.chromaticism),
            ornament: unit(self.ornament),
            mode_dark: unit(self.mode_dark),
            syncopation: unit(self.syncopation),
            harmonic_rate: unit(self.harmonic_rate),
            refinement: unit(self.refinement),
            register: unit(self.register),
            grit: unit(self.grit),
        }
    }
}

impl Default for Parameters {
    /// [`Parameters::DEFAULT`], for the reason stated there.
    fn default() -> Self {
        Self::DEFAULT
    }
}
