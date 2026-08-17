//! One sounding voice: a sample being read, or an oscillator, plus an envelope.

use crate::synth::envelope::{Envelope, Shape};
use crate::synth::gens::Articulation;

/// The shape a bankless voice makes.
///
/// The fallback for a synthesizer with no `SoundFont`: enough to hear the notes,
/// nowhere near enough to hear the music. A sawtooth through the composer's
/// output is the sound of checking that the harmony works, and it is what
/// `tests/render.rs` renders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub enum Waveform {
    /// One partial, and the quietest thing to test against.
    Sine,
    /// Odd partials falling off quickly: a stopped pipe.
    Triangle,
    /// Every partial: the reed, and the default.
    #[default]
    Sawtooth,
    /// Odd partials only: a clarinet at a distance and a buzz up close.
    Square,
}

impl Waveform {
    /// The value of this waveform at `phase`, which runs `0.0 ..= 1.0`.
    fn at(self, phase: f32) -> f32 {
        match self {
            Self::Sine => libm::sinf(phase * core::f32::consts::TAU),
            Self::Triangle => 4.0 * libm::fabsf(phase - 0.5) - 1.0,
            Self::Sawtooth => phase * 2.0 - 1.0,
            Self::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }
}

/// Where a voice's samples come from.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    /// A sample in the bank, read at a fractional position.
    Sampled {
        /// Which sample.
        index: usize,
        /// Where in it, in frames.
        position: f64,
        /// How far to advance a frame.
        increment: f64,
        /// The loop, in frames, when it loops.
        looping: Option<(f64, f64)>,
    },
    /// An oscillator, for a synthesizer with no bank.
    Oscillated {
        /// Which shape.
        waveform: Waveform,
        /// Where in the cycle, `0.0 ..= 1.0`.
        phase: f32,
        /// How far to advance a frame.
        increment: f32,
    },
}

/// One sounding voice.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Voice {
    /// Which channel started it.
    pub(crate) channel: u8,
    /// Which key it is sounding.
    pub(crate) key: u8,
    /// The group in which one note cuts another off; zero is no group.
    pub(crate) exclusive: i32,
    source: Source,
    envelope: Envelope,
    left: f32,
    right: f32,
    finished: bool,
}

/// The frequency of MIDI key 69, in hertz.
const A4: f32 = 440.0;

/// Equal-power pan gains for `pan` running left to right.
fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * 0.5 * core::f32::consts::FRAC_PI_2;
    (libm::cosf(angle), libm::sinf(angle))
}

impl Voice {
    /// A voice reading a bank sample.
    pub(crate) fn sampled(
        channel: u8,
        key: u8,
        articulation: Articulation,
        sample_rate: u32,
        source_rate: u32,
        amplitude: f32,
    ) -> Self {
        let ratio = f64::from(source_rate) / f64::from(sample_rate.max(1));
        let increment = ratio * f64::from(libm::exp2f(articulation.pitch / 12.0));
        let (left, right) = pan_gains(articulation.pan);
        let gain = amplitude * articulation.gain;
        Self {
            channel,
            key,
            exclusive: articulation.exclusive,
            source: Source::Sampled {
                index: articulation.sample,
                position: 0.0,
                increment,
                looping: articulation
                    .looping
                    .map(|(start, end)| (f64::from(start), f64::from(end))),
            },
            envelope: Envelope::new(articulation.shape, sample_rate),
            left: gain * left,
            right: gain * right,
            finished: false,
        }
    }

    /// A voice running an oscillator.
    pub(crate) fn oscillated(
        channel: u8,
        key: u8,
        waveform: Waveform,
        sample_rate: u32,
        amplitude: f32,
        pan: f32,
    ) -> Self {
        let hertz = A4 * libm::exp2f((f32::from(key) - 69.0) / 12.0);
        let (left, right) = pan_gains(pan);
        Self {
            channel,
            key,
            exclusive: 0,
            source: Source::Oscillated {
                waveform,
                phase: 0.0,
                increment: hertz / crate::num::of_u32(sample_rate.max(1)),
            },
            envelope: Envelope::new(Shape::default(), sample_rate),
            left: amplitude * left,
            right: amplitude * right,
            finished: false,
        }
    }

    /// Whether the voice has fallen silent and its slot can be reused.
    pub(crate) const fn is_finished(&self) -> bool {
        self.finished
    }

    /// Which bank sample this voice reads, or `None` for an oscillator.
    ///
    /// The mixer owns the bank, so it is the mixer that turns this into audio;
    /// a voice holding a borrow of the bank would stop the mixer from ever
    /// swapping one.
    pub(crate) const fn sample_index(&self) -> Option<usize> {
        match self.source {
            Source::Sampled { index, .. } => Some(index),
            Source::Oscillated { .. } => None,
        }
    }

    /// Whether the key has been let go.
    pub(crate) const fn is_releasing(&self) -> bool {
        self.envelope.is_releasing()
    }

    /// Lets the key go.
    pub(crate) const fn release(&mut self) {
        self.envelope.release();
    }

    /// Cuts the voice off over `seconds` rather than dropping it mid-waveform,
    /// which would click.
    pub(crate) const fn cut(&mut self, seconds: f32) {
        self.envelope.cut(seconds);
    }

    /// Adds this voice into the interleaved-stereo block `out`.
    ///
    /// `pcm` is the sample this voice reads, which the mixer looks up because it
    /// owns the bank; a sampled voice handed `None` finishes rather than
    /// sounding, which is what a bank swapped underneath a held note deserves.
    /// `channel` is the channel's own gain, applied last.
    pub(crate) fn render(&mut self, out: &mut [f32], pcm: Option<&[i16]>, channel: f32) {
        if self.finished {
            return;
        }
        let left = self.left * channel;
        let right = self.right * channel;
        for frame in out.chunks_exact_mut(2) {
            let Some(value) = self.step(pcm) else {
                self.finished = true;
                return;
            };
            let gain = self.envelope.next();
            if let [one, other] = frame {
                *one += value * gain * left;
                *other += value * gain * right;
            }
            if self.envelope.is_finished() {
                self.finished = true;
                return;
            }
        }
    }

    /// The next sample from this voice's source, or `None` when it has run out.
    fn step(&mut self, pcm: Option<&[i16]>) -> Option<f32> {
        match &mut self.source {
            Source::Sampled {
                position,
                increment,
                looping,
                ..
            } => {
                let data = pcm?;
                if data.is_empty() {
                    return None;
                }
                // Linear interpolation between the two straddling frames.
                // Reading through `get` costs the same bounds check indexing
                // already did and treats a position past the end as silence
                // rather than as a panic.
                let whole = libm::floor(*position);
                let index = crate::num::frame(whole);
                let first = data.get(index).copied().unwrap_or(0);
                let second = data.get(index + 1).copied().unwrap_or(first);
                let fraction = crate::num::narrow(*position - whole);
                let value = (f32::from(first) + (f32::from(second) - f32::from(first)) * fraction)
                    / 32_768.0;
                *position += *increment;
                match looping {
                    Some((start, end)) if *position >= *end => *position -= *end - *start,
                    _ => {
                        if index + 1 >= data.len() {
                            return None;
                        }
                    }
                }
                Some(value)
            }
            Source::Oscillated {
                waveform,
                phase,
                increment,
            } => {
                let value = waveform.at(*phase);
                *phase += *increment;
                while *phase >= 1.0 {
                    *phase -= 1.0;
                }
                Some(value)
            }
        }
    }
}
