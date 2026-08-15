//! How a sound is made, as numbers rather than as a recording.
//!
//! The seam against `voice.rs` is that nothing here is per-sample: a [`Timbre`]
//! is what a game authors, and a voice is what one becomes once a device has
//! said how fast it samples.

/// How a sound is made, as numbers rather than as a recording.
///
/// A struck object rings at a pitch and dies away, and the two things that make
/// one sound different from another are how fast it dies and how much is going
/// on above the fundamental. That is all this has, and it is enough for a
/// thud, a click, a chime and a thump.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timbre {
    /// The fundamental, in hertz, before a cue's pitch is applied.
    pub hertz: f32,
    /// How long the sound takes to fall to a thousandth of its loudest, in
    /// seconds.
    pub decay: f32,
    /// How much of the octave above the fundamental is mixed in, from none to
    /// as much as the fundamental.
    ///
    /// This is the whole of the timbre control. A second partial an octave up
    /// is what separates a wooden knock from a pure tone, and anything more
    /// expressive is a recording rather than four numbers.
    pub bite: f32,
    /// How long the sound takes to reach its loudest, in seconds.
    ///
    /// Not zero, and that is the point: a waveform that starts at full
    /// amplitude on its first sample is a step, and a step is a click that
    /// every sound in the mix shares. A millisecond is short enough to be
    /// percussive and long enough not to snap.
    pub attack: f32,
}

impl Timbre {
    /// A short percussive knock at `hertz`.
    #[must_use]
    pub const fn knock(hertz: f32) -> Self {
        Self {
            hertz,
            decay: 0.25,
            bite: 0.4,
            attack: 0.001,
        }
    }

    /// How fast it dies away.
    #[must_use]
    pub const fn with_decay(self, decay: f32) -> Self {
        Self { decay, ..self }
    }

    /// How much of the octave above is in it.
    #[must_use]
    pub const fn with_bite(self, bite: f32) -> Self {
        Self { bite, ..self }
    }

    /// How long it takes to reach its loudest.
    #[must_use]
    pub const fn with_attack(self, attack: f32) -> Self {
        Self { attack, ..self }
    }
}

/// A knock at concert A.
///
/// Hand-written because a derived `Default` is four zeroes, and a timbre of no
/// pitch that decays in no time is silence rather than a sound.
impl Default for Timbre {
    /// A knock at concert A.
    fn default() -> Self {
        Self::knock(440.0)
    }
}
