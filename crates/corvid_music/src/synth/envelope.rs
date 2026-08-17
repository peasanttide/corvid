//! The volume envelope: delay, attack, hold, decay, sustain, release.
//!
//! The specification's six segments, articulated per sample. The attack is
//! linear where the specification calls for a convex curve, which is audible
//! only on a very slow attack and is the one place this implementation is
//! deliberately simpler than the document; everything else is the shape a bank
//! asks for.

/// How a note is shaped over time, in seconds and in linear gain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shape {
    /// Silence before anything happens.
    pub(crate) delay: f32,
    /// Time from silence to full.
    pub(crate) attack: f32,
    /// Time held at full.
    pub(crate) hold: f32,
    /// Time from full down to the sustain level.
    pub(crate) decay: f32,
    /// The level held while the key is down, from `0.0` to `1.0`.
    pub(crate) sustain: f32,
    /// Time from wherever it is down to silence, once released.
    pub(crate) release: f32,
}

impl Default for Shape {
    /// A short attack, no decay and a short release: an envelope that will not
    /// click and will not colour anything.
    fn default() -> Self {
        Self {
            delay: 0.0,
            attack: 0.002,
            hold: 0.0,
            decay: 0.0,
            sustain: 1.0,
            release: 0.08,
        }
    }
}

/// Which segment a voice is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Segment {
    Delay,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    Done,
}

/// The level at which a release is called finished.
///
/// Minus eighty decibels: below the noise floor of anything that will play the
/// result, and reached in finite time by a geometric decay that would otherwise
/// approach zero forever.
const FLOOR: f32 = 1e-4;

/// A running envelope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Envelope {
    segment: Segment,
    level: f32,
    /// Seconds spent in the current segment.
    elapsed: f32,
    /// One sample, in seconds.
    step: f32,
    shape: Shape,
    /// The level the release started from, so a note released during its attack
    /// falls from where it actually was.
    released_from: f32,
}

impl Envelope {
    /// An envelope of `shape` running at `sample_rate`.
    pub(crate) fn new(shape: Shape, sample_rate: u32) -> Self {
        Self {
            segment: if shape.delay > 0.0 {
                Segment::Delay
            } else {
                Segment::Attack
            },
            level: 0.0,
            elapsed: 0.0,
            step: 1.0 / crate::num::of_u32(sample_rate.max(1)),
            shape,
            released_from: 1.0,
        }
    }

    /// Whether the envelope has fallen silent and the voice can be reclaimed.
    pub(crate) const fn is_finished(&self) -> bool {
        matches!(self.segment, Segment::Done)
    }

    /// Whether the key has been let go.
    pub(crate) const fn is_releasing(&self) -> bool {
        matches!(self.segment, Segment::Release | Segment::Done)
    }

    /// Lets the key go.
    pub(crate) const fn release(&mut self) {
        if !self.is_releasing() {
            self.released_from = self.level;
            self.segment = Segment::Release;
            self.elapsed = 0.0;
        }
    }

    /// Cuts the voice off over `seconds`, for stealing it without a click.
    pub(crate) const fn cut(&mut self, seconds: f32) {
        self.shape.release = seconds;
        self.release();
    }

    /// The gain for this sample, advancing one sample.
    pub(crate) fn next(&mut self) -> f32 {
        let elapsed = self.elapsed;
        self.elapsed += self.step;
        match self.segment {
            Segment::Delay => {
                if elapsed >= self.shape.delay {
                    self.enter(Segment::Attack);
                }
                0.0
            }
            Segment::Attack => {
                if self.shape.attack <= 0.0 {
                    self.level = 1.0;
                    self.enter(Segment::Hold);
                } else {
                    self.level = (elapsed / self.shape.attack).min(1.0);
                    if self.level >= 1.0 {
                        self.enter(Segment::Hold);
                    }
                }
                self.level
            }
            Segment::Hold => {
                self.level = 1.0;
                if elapsed >= self.shape.hold {
                    self.enter(Segment::Decay);
                }
                1.0
            }
            Segment::Decay => {
                let sustain = self.shape.sustain.clamp(0.0, 1.0);
                if self.shape.decay <= 0.0 {
                    self.level = sustain;
                    self.enter(Segment::Sustain);
                } else {
                    let done = (elapsed / self.shape.decay).min(1.0);
                    self.level = 1.0 - done * (1.0 - sustain);
                    if done >= 1.0 {
                        self.enter(Segment::Sustain);
                    }
                }
                self.level
            }
            Segment::Sustain => {
                self.level = self.shape.sustain.clamp(0.0, 1.0);
                self.level
            }
            Segment::Release => {
                if self.shape.release <= 0.0 {
                    self.level = 0.0;
                    self.enter(Segment::Done);
                    return 0.0;
                }
                // Geometric in amplitude, which is linear in decibels and is
                // what a release sounds like rather than a straight line to
                // zero.
                let done = (elapsed / self.shape.release).min(1.0);
                self.level = self.released_from * libm::powf(FLOOR, done);
                if done >= 1.0 || self.level <= FLOOR {
                    self.level = 0.0;
                    self.enter(Segment::Done);
                }
                self.level
            }
            Segment::Done => 0.0,
        }
    }

    /// Moves to `segment` and restarts its clock.
    const fn enter(&mut self, segment: Segment) {
        self.segment = segment;
        self.elapsed = 0.0;
    }
}
