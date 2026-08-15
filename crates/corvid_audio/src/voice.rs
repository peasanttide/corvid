//! The one voice playing a cue.
//!
//! There are no recordings to load here, so a sound is described rather than
//! loaded: a [`Timbre`] is four numbers, written down beside this, and a
//! [`Voice`] is the oscillator that turns them into samples. The seam is the
//! sample rate: everything here needs one and nothing in `timbre.rs` does.

use crate::Timbre;

/// One cue being played: an oscillator, an envelope, and how loud it is.
///
/// # Why it holds a phase and an amplitude rather than a time
///
/// Everything that advances here is one multiply or one add per sample, so
/// filling a buffer costs no library call, no allocation and no branch that
/// could be a division by zero. A voice written as a function of elapsed time
/// would want a sine and an exponential per sample; this wants one sine, and
/// the amplitude is a running product of a factor worked out once when the
/// voice started.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Voice {
    /// Where the fundamental is in its cycle, in radians.
    phase: f32,
    /// How far the phase moves per sample.
    step: f32,
    /// How loud the envelope is now.
    envelope: f32,
    /// What the envelope is multiplied by per sample while it is decaying.
    fade: f32,
    /// What is added to the envelope per sample while it is still rising.
    rise: f32,
    /// Whether the envelope is still rising.
    attacking: bool,
    /// How loud the whole voice is, from the cue's gain and the listener's.
    gain: f32,
    /// How much of the octave above the fundamental is mixed in.
    bite: f32,
    /// Whether this voice is playing at all.
    playing: bool,
}

/// Below this the envelope is inaudible and the voice is freed.
///
/// A sixteen-bit sample cannot represent anything smaller than about 3e-5 of
/// full scale, so a voice quieter than this is contributing nothing a device
/// can play and is holding a slot a new cue could use.
const AUDIBLE: f32 = 1.0e-5;

impl Voice {
    /// Starts this voice on `timbre` at `gain`, sampled at `rate` hertz.
    ///
    /// `rate` and every field of `timbre` are taken as they come and made
    /// finite and positive here rather than trusted: this runs on the device's
    /// own thread, where a zero divisor or a `NaN` would be a silence or a
    /// scream that no test on the game's thread could have caught.
    pub(crate) fn start(&mut self, timbre: Timbre, gain: f32, rate: f32) {
        let rate = if rate.is_finite() && rate >= 1.0 {
            rate
        } else {
            1.0
        };
        let hertz = clamp(timbre.hertz, 1.0, rate / 2.0);
        // A decay is quoted as the time to a thousandth, so the per-sample
        // factor is the one whose `rate * decay`-th power is a thousandth.
        let decay = clamp(timbre.decay, 0.001, 60.0);
        let attack = clamp(timbre.attack, 1.0 / rate, decay);

        self.phase = 0.0;
        self.step = core::f32::consts::TAU * hertz / rate;
        self.envelope = 0.0;
        self.fade = (-6.907_755 / (decay * rate)).exp();
        self.rise = 1.0 / (attack * rate);
        self.attacking = true;
        self.gain = clamp(gain, 0.0, 1.0);
        self.bite = clamp(timbre.bite, 0.0, 1.0);
        self.playing = true;
    }

    /// Whether this voice is making any sound.
    #[must_use]
    pub(crate) const fn playing(&self) -> bool {
        self.playing
    }

    /// How little of this voice is left, which is what a mixer with no free
    /// voice steals by.
    ///
    /// A voice that is still attacking answers its `gain` rather than its
    /// envelope, because the envelope of a voice that has not been sampled yet
    /// is zero: rank it by where it is now and every note started in this
    /// callback is the quietest thing in the pool, so a full batch of cues
    /// steals the slot it filled one instruction ago and only the last of them
    /// is ever heard. The envelope means "closest to being finished" only once
    /// it is falling; while it is rising, where it is *going* is what says how
    /// much of the voice a steal would cut off.
    #[must_use]
    pub(crate) const fn loudness(&self) -> f32 {
        if !self.playing {
            return 0.0;
        }
        if self.attacking {
            self.gain
        } else {
            self.envelope * self.gain
        }
    }

    /// The next sample, and the envelope advanced past it.
    ///
    /// Zero for a voice that is not playing, so a caller need not ask first.
    pub(crate) fn next_sample(&mut self) -> f32 {
        if !self.playing {
            return 0.0;
        }
        let sample = (self.phase.sin() + self.bite * (2.0 * self.phase).sin()) * self.envelope;

        self.phase += self.step;
        if self.phase >= core::f32::consts::TAU {
            self.phase -= core::f32::consts::TAU;
        }
        if self.attacking {
            self.envelope += self.rise;
            if self.envelope >= 1.0 {
                self.envelope = 1.0;
                self.attacking = false;
            }
        } else {
            self.envelope *= self.fade;
            if self.envelope < AUDIBLE {
                self.playing = false;
            }
        }

        // Divided by one and a bite, so a voice with the octave mixed in is
        // never louder than one without it -- otherwise `bite` would be a volume
        // control wearing a timbre's name. It is somewhat *quieter*, because
        // the two partials do not peak together: at full bite the loudest
        // sample of a 200 Hz knock measures 0.80 against 0.99 without, which
        // `tests` below pins down.
        sample * self.gain / (1.0 + self.bite)
    }
}

/// `value` inside `low ..= high`, with anything that is not finite becoming
/// `low`.
///
/// An infinity goes to the bottom rather than to the top, which is the opposite
/// of what an arithmetic reading would say and is the right answer for
/// everything here: a frequency, a decay and an attack that arrived as
/// infinities are a device or a caller that has malfunctioned, and the quietest
/// and slowest interpretation of a malfunction is the one that does not scream.
///
/// `f32::clamp` panics if the bounds are the wrong way round or are `NaN`, and
/// this crate denies panics because the caller it protects is an audio
/// callback: a device thread that unwound would take the stream with it and
/// leave a game running in silence.
use corvid_float::clamp_finite as clamp;

#[cfg(test)]
mod tests {
    //! What a voice does that cannot be heard by looking at it.

    #![allow(
        clippy::panic,
        reason = "a failed assertion in a test is a failed test, which is what a test is for"
    )]
    #![allow(
        clippy::float_cmp,
        reason = "the comparisons here are against exact zero and against literals a clamp returned unchanged, which is what is being asserted -- a silent voice that produced 1e-30 instead of 0.0 would be a bug this crate has no other way to see"
    )]

    use super::{AUDIBLE, Timbre, Voice, clamp};

    /// A rate to sample at, in hertz.
    const RATE: f32 = 48_000.0;

    #[test]
    fn a_voice_that_was_never_started_is_silent() {
        let mut voice = Voice::default();
        assert!(!voice.playing());
        assert_eq!(voice.next_sample(), 0.0);
        assert_eq!(voice.loudness(), 0.0);
    }

    #[test]
    fn a_voice_starts_from_silence_rather_than_from_full_amplitude() {
        // The first sample of a voice is what every cue in the mix has in
        // common, so a step there is a click on every sound at once.
        let mut voice = Voice::default();
        voice.start(Timbre::knock(440.0), 1.0, RATE);
        assert_eq!(voice.next_sample(), 0.0);
        let early = voice.next_sample().abs();
        assert!(early < 0.01, "the second sample was already {early}");
    }

    #[test]
    fn a_voice_stops_by_itself_and_frees_its_slot() {
        // A mixer with a fixed pool depends on this: a voice that never
        // finished would hold its slot until it was stolen.
        let mut voice = Voice::default();
        voice.start(Timbre::knock(440.0).with_decay(0.01), 1.0, RATE);
        let mut samples = 0;
        while voice.playing() && samples < 48_000 {
            let _ = voice.next_sample();
            samples += 1;
        }
        assert!(!voice.playing(), "still playing after a whole second");
        // A decay is quoted as the time to a thousandth, and the voice is freed
        // at a hundred-thousandth, so a hundredth of a second of decay is about
        // one and two thirds of that -- call it three, which is well short of
        // the second the loop above allowed.
        assert!(samples < 3 * 480, "took {samples} samples to fall silent");
    }

    #[test]
    fn a_quieter_cue_is_quieter_all_the_way_through() {
        let peak = |gain: f32| {
            let mut voice = Voice::default();
            voice.start(Timbre::knock(440.0), gain, RATE);
            let mut loudest: f32 = 0.0;
            for _ in 0..4_800 {
                loudest = loudest.max(voice.next_sample().abs());
            }
            loudest
        };
        let full = peak(1.0);
        let half = peak(0.5);
        assert!(full > 0.1, "a cue at full gain peaked at only {full}");
        let ratio = half / full;
        assert!(
            (ratio - 0.5).abs() < 0.02,
            "half the gain was {ratio} of the amplitude",
        );
    }

    #[test]
    fn the_bite_changes_the_shape_without_making_it_louder() {
        // Otherwise `bite` would be a second gain control wearing a timbre's
        // name, and a game that wanted a woodier knock would get a louder one.
        // It is quieter rather than equal, because two partials an octave apart
        // do not reach their peaks together; what is asserted is that the
        // difference is a shading rather than a volume control.
        let sample_at = |bite: f32, at: usize| {
            let mut voice = Voice::default();
            voice.start(Timbre::knock(200.0).with_bite(bite), 1.0, RATE);
            let mut last = 0.0;
            for _ in 0..=at {
                last = voice.next_sample();
            }
            last
        };
        let peak = |bite: f32| {
            let mut voice = Voice::default();
            voice.start(Timbre::knock(200.0).with_bite(bite), 1.0, RATE);
            let mut loudest: f32 = 0.0;
            for _ in 0..4_800 {
                loudest = loudest.max(voice.next_sample().abs());
            }
            loudest
        };
        let plain = peak(0.0);
        let bitten = peak(1.0);
        assert!(
            bitten <= plain,
            "adding the octave made it louder: {plain} became {bitten}",
        );
        assert!(
            bitten > plain * 0.7,
            "adding the octave halved it: {plain} became {bitten}",
        );
        // And it is a different sound, which the peak alone would not say: a
        // quarter of the way through a cycle the fundamental is at its top and
        // the octave is crossing zero going the other way.
        let quarter = 48_000 / 200 / 4;
        assert!(
            (sample_at(0.0, quarter) - sample_at(1.0, quarter)).abs() > 0.05,
            "the two timbres agree where they should differ",
        );
    }

    #[test]
    fn a_device_that_reports_nonsense_is_survived() {
        // Every one of these reaches `start` from a platform rather than from
        // this workspace, and every one of them would otherwise be a division
        // by zero, an infinite phase step, or a `NaN` smeared across the mix.
        for (rate, timbre) in [
            (0.0, Timbre::knock(440.0)),
            (f32::NAN, Timbre::knock(440.0)),
            (RATE, Timbre::knock(f32::INFINITY)),
            (RATE, Timbre::knock(0.0).with_decay(0.0)),
            (RATE, Timbre::knock(440.0).with_attack(f32::NAN)),
        ] {
            let mut voice = Voice::default();
            voice.start(timbre, 1.0, rate);
            for _ in 0..1_000 {
                let sample = voice.next_sample();
                assert!(
                    sample.is_finite(),
                    "{timbre:?} at {rate} Hz produced {sample}"
                );
                assert!(
                    sample.abs() <= 1.0,
                    "{timbre:?} at {rate} Hz produced {sample}"
                );
            }
        }
    }

    #[test]
    fn a_voice_is_stolen_by_how_loud_it_is_rather_than_by_how_old_it_is() {
        let mut loud = Voice::default();
        let mut quiet = Voice::default();
        loud.start(Timbre::knock(440.0), 1.0, RATE);
        quiet.start(Timbre::knock(440.0), 0.1, RATE);
        for _ in 0..480 {
            let _ = loud.next_sample();
            let _ = quiet.next_sample();
        }
        assert!(loud.loudness() > quiet.loudness());
        assert!(quiet.loudness() > AUDIBLE);
    }

    #[test]
    fn the_clamp_answers_the_low_end_for_anything_that_is_not_finite() {
        assert_eq!(clamp(f32::NAN, 2.0, 5.0), 2.0);
        assert_eq!(clamp(f32::NEG_INFINITY, 2.0, 5.0), 2.0);
        assert_eq!(clamp(f32::INFINITY, 2.0, 5.0), 2.0);
        assert_eq!(clamp(3.0, 2.0, 5.0), 3.0);
        assert_eq!(clamp(9.0, 2.0, 5.0), 5.0);
    }
}
