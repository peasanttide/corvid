//! The one source of randomness, and it is seeded by hand.

use corvid_float::consts::TAU;
use corvid_glm::Vec3;

/// The multiplier of the LCG underneath, from Knuth by way of PCG.
const MULTIPLIER: u64 = 6_364_136_223_846_793_005;

/// The increment, which has to be odd for the LCG to reach its full period.
const INCREMENT: u64 = 1_442_695_040_888_963_407;

/// A seeded stream of pseudo-random numbers: PCG-XSH-RR, 64 bits of state.
///
/// **Nothing in this crate reads a clock, and this is the only place a number
/// arrives from nowhere.** Two runs of the same emitters, stepped with the same
/// times, from a [`Rng::new`] with the same seed, produce the same particles in
/// the same order -- which is what lets a particle effect be a golden test
/// rather than something a human looks at and says looks about right.
///
/// PCG rather than a xorshift because the low bits of an LCG are famously poor
/// and the permutation is what fixes them, and because sixty-four bits of state
/// is a struct a [`crate::System`] can hold by value.
///
/// ```
/// use corvid_particle::Rng;
///
/// let mut once = Rng::new(17_890_428);
/// let mut again = Rng::new(17_890_428);
/// assert_eq!(once.next_u32(), again.next_u32());
///
/// // And a value in `0.0 .. 1.0`, never 1.0 itself.
/// let unit = once.unit();
/// assert!((0.0..1.0).contains(&unit));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Rng {
    /// The LCG state, which is advanced before every output.
    state: u64,
}

impl Rng {
    /// A stream from a seed.
    ///
    /// Every seed is a valid one, including zero: the state is advanced once
    /// here so that a zero seed does not spend its first output on the zero the
    /// LCG would still be sitting on.
    #[must_use]
    #[inline]
    pub const fn new(seed: u64) -> Self {
        let mut rng = Self { state: seed };
        rng.state = rng.state.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT);
        rng
    }

    /// The next thirty-two bits.
    #[must_use]
    #[inline]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the narrowing is the permutation: XSH takes the middle word of the state and RR the rotation from above it"
    )]
    pub const fn next_u32(&mut self) -> u32 {
        let previous = self.state;
        self.state = previous.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT);
        // XSH-RR: xor the high bits down over the low ones, take the middle
        // word, and rotate it by the bits above everything the word came from.
        // The rotate is what makes the output stream pass the tests the raw LCG
        // fails, and it costs one instruction.
        let xored = (((previous >> 18) ^ previous) >> 27) as u32;
        let rotation = (previous >> 59) as u32;
        xored.rotate_right(rotation)
    }

    /// A value in `0.0 .. 1.0`, uniform, never reaching one.
    ///
    /// Twenty-four bits of the output become the mantissa and the rest is
    /// discarded, which is the whole of the precision an `f32` in the unit
    /// interval has.
    #[must_use]
    #[inline]
    #[expect(
        clippy::cast_precision_loss,
        reason = "the value is masked to 24 bits, which an f32 holds exactly"
    )]
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.0
    }

    /// A value in `low .. high`, uniform.
    ///
    /// Reversed bounds are not an error and are not sorted either: the result
    /// is `low` plus a fraction of the signed difference, so a `high` below a
    /// `low` simply counts down from `low`.
    #[must_use]
    #[inline]
    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }

    /// A unit vector, uniform over the sphere.
    ///
    /// Uniform by area rather than by angle, which is Archimedes' theorem: a
    /// uniform height on the axis and a uniform turn around it land uniformly
    /// on the sphere. Sampling two angles instead would crowd the poles, and a
    /// burst that is denser straight up than sideways is the artifact.
    #[must_use]
    pub fn direction(&mut self) -> Vec3 {
        let z = self.range(-1.0, 1.0);
        let turn = self.range(0.0, TAU);
        // Clamped because `z * z` can round to just above one at the poles, and
        // the root of a negative is a `NaN` that would spread into a position.
        let radius = corvid_float::sqrt(corvid_float::clamp(1.0 - z * z, 0.0, 1.0));
        Vec3::new(
            radius * corvid_float::cos(turn),
            radius * corvid_float::sin(turn),
            z,
        )
    }
}
