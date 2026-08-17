//! What varies over a particle's life: a span of numbers and a span of colours.

use corvid_color::LinearRgba;
use corvid_fixed::Factor32;

use crate::Rng;

/// A closed span of numbers a value is drawn from at birth.
///
/// Every random quantity an emitter has is one of these, so an effect that
/// wants a fixed value writes [`Range::exactly`] rather than reaching for a
/// second set of fields.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Range {
    /// The value at the bottom of the span.
    pub low: f32,
    /// The value at the top of it.
    pub high: f32,
}

impl Range {
    /// A span between two values.
    #[must_use]
    #[inline]
    pub const fn new(low: f32, high: f32) -> Self {
        Self { low, high }
    }

    /// The span containing one value, which is what a quantity that does not
    /// vary is written as.
    #[must_use]
    #[inline]
    pub const fn exactly(value: f32) -> Self {
        Self {
            low: value,
            high: value,
        }
    }

    /// A value from the span, uniform.
    ///
    /// Exactly one draw from `rng`, whatever the span is -- including
    /// [`exactly`](Self::exactly), where the draw is thrown away. That costs a
    /// few nanoseconds and buys the property the whole crate rests on: the
    /// number of draws a spawn makes does not depend on the values in the
    /// emitter, so widening a range in an editor does not renumber every
    /// particle that comes after it.
    #[must_use]
    #[inline]
    pub fn sample(self, rng: &mut Rng) -> f32 {
        rng.range(self.low, self.high)
    }
}

/// The colour of a particle over its life: four stops, evenly spaced.
///
/// Four rather than two because the palette the printed period wants is three
/// inks and an exit -- lead-tin yellow at the core, vermilion as it cools, bone
/// black as it chars, and nothing at all -- and two stops cannot say that
/// without a second emitter. Four rather than a growable list because a ramp is
/// then [`Copy`], costs sixty-four bytes, and can sit inside an emitter that a
/// caller pokes at every frame.
///
/// The stops are [`corvid_color::LinearRgba`], which is fixed point, so a ramp
/// is [`Eq`] and [`core::hash::Hash`] and a golden can freeze one. The float
/// appears where a particle becomes an [`crate::Instance`] and not before.
///
/// ```
/// use corvid_color::LinearRgba;
/// use corvid_particle::ColorRamp;
///
/// let smoke = ColorRamp::fade(LinearRgba::WHITE, LinearRgba::TRANSPARENT);
/// assert_eq!(smoke.sample(0.0), LinearRgba::WHITE);
/// assert_eq!(smoke.sample(1.0), LinearRgba::TRANSPARENT);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColorRamp {
    /// The four stops, at zero, a third, two thirds and one of the life.
    pub stops: [LinearRgba; 4],
}

impl ColorRamp {
    /// How many segments the stops make, which is one fewer than there are.
    const SEGMENTS: f32 = 3.0;

    /// A ramp from its four stops.
    #[must_use]
    #[inline]
    pub const fn new(stops: [LinearRgba; 4]) -> Self {
        Self { stops }
    }

    /// One colour for the whole life.
    #[must_use]
    #[inline]
    pub const fn solid(color: LinearRgba) -> Self {
        Self::new([color; 4])
    }

    /// A straight fade from birth to death, the two middle stops on the line
    /// between them, so that this and [`new`](Self::new) sample alike.
    #[must_use]
    #[inline]
    pub const fn fade(from: LinearRgba, to: LinearRgba) -> Self {
        Self::new([
            from,
            from.lerp(to, Factor32::from_f64(1.0 / 3.0)),
            from.lerp(to, Factor32::from_f64(2.0 / 3.0)),
            to,
        ])
    }

    /// The colour at `t`, where zero is birth and one is death.
    ///
    /// Values outside the unit interval clamp and a `NaN` lands on the first
    /// stop, because a colour is what a frame is drawn with and the failure a
    /// renderer cannot recover from is a vertex it cannot place rather than a
    /// particle that is the wrong red.
    #[must_use]
    pub fn sample(self, t: f32) -> LinearRgba {
        let scaled = corvid_float::clamp(t, 0.0, 1.0) * Self::SEGMENTS;
        // Destructured rather than indexed: the segment is a choice between
        // three pairs, and written this way there is no index for the compiler
        // to have to prove is in bounds. The last segment is closed at both
        // ends, so a `t` of exactly one reads it at weight one rather than
        // asking for a fourth segment that is not there.
        let [birth, early, late, death] = self.stops;
        let (from, to, weight) = if scaled < 1.0 {
            (birth, early, scaled)
        } else if scaled < 2.0 {
            (early, late, scaled - 1.0)
        } else {
            (late, death, scaled - 2.0)
        };
        from.lerp(to, Factor32::from_f64(f64::from(weight)))
    }
}

/// White for the whole life, which is the ramp an emitter starts with.
impl Default for ColorRamp {
    #[inline]
    fn default() -> Self {
        Self::solid(LinearRgba::WHITE)
    }
}
