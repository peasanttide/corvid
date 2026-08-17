//! The one source of chance in the crate.
//!
//! Everything the composer decides at random goes through here, because "the
//! same seed and the same parameters give the same bar" is the property that
//! makes any of it testable. A `Rng` holds sixty-four bits of state and is
//! advanced by `SplitMix64`, whose output is a fixed function of that state
//! alone -- no address, no clock, no hasher's random seed.

/// A seeded stream of chance.
///
/// `SplitMix64`: the state is advanced by an odd increment and the output is a
/// finalizing mix of it. Chosen for being small enough to write down and check
/// -- the whole generator is six lines -- and for having no hidden state, so a
/// composer restored from a seed and a bar count is the composer that produced
/// that bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Rng {
    state: u64,
}

/// The golden-ratio increment `SplitMix64` steps its state by.
const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

impl Rng {
    /// A stream from `seed`.
    pub(crate) const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next sixty-four bits.
    pub(crate) const fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GAMMA);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A float in `0.0 ..= 1.0`, exclusive of one.
    ///
    /// The top twenty-four bits, scaled: an `f32` has twenty-four bits of
    /// mantissa, so taking more would be discarded by the conversion and taking
    /// fewer would leave gaps in the range.
    pub(crate) fn unit(&mut self) -> f32 {
        let bits = u32::try_from(self.next_u64() >> 40).unwrap_or(0);
        crate::num::of_u32(bits) / 16_777_216.0
    }

    /// An index below `count`, or `None` when `count` is zero.
    pub(crate) fn below(&mut self, count: usize) -> Option<usize> {
        if count == 0 {
            return None;
        }
        let width = u64::try_from(count).unwrap_or(u64::MAX);
        usize::try_from(self.next_u64() % width).ok()
    }

    /// True with probability `chance`, which is clamped into `0.0 ..= 1.0`.
    pub(crate) fn chance(&mut self, chance: f32) -> bool {
        self.unit() < chance
    }

    /// Picks an index in proportion to `weights`, ignoring negative entries.
    ///
    /// Answers `None` for an empty slice or one that sums to nothing, so a
    /// caller with no candidates has to say what it wants instead rather than
    /// being handed index zero as though it had been chosen.
    pub(crate) fn weighted(&mut self, weights: &[f32]) -> Option<usize> {
        let total: f32 = weights.iter().copied().filter(|w| *w > 0.0).sum();
        if total <= 0.0 || !total.is_finite() {
            return None;
        }
        let mut target = self.unit() * total;
        for (index, weight) in weights.iter().enumerate() {
            if *weight <= 0.0 {
                continue;
            }
            target -= *weight;
            if target <= 0.0 {
                return Some(index);
            }
        }
        // Reached only when rounding leaves `target` a hair above zero at the
        // end; the last positive weight is the one the draw landed in.
        weights.iter().rposition(|w| *w > 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::Rng;

    #[test]
    fn a_seed_reproduces_its_stream() {
        let mut first = Rng::new(7);
        let mut second = Rng::new(7);
        for _ in 0..64 {
            assert_eq!(first.next_u64(), second.next_u64());
        }
    }

    #[test]
    fn units_stay_inside_the_unit_interval() {
        let mut rng = Rng::new(1);
        for _ in 0..4096 {
            let value = rng.unit();
            assert!((0.0..1.0).contains(&value), "{value}");
        }
    }

    #[test]
    fn a_weight_of_zero_is_never_drawn() {
        let mut rng = Rng::new(3);
        for _ in 0..256 {
            assert_eq!(rng.weighted(&[0.0, 1.0, 0.0]), Some(1));
        }
        assert_eq!(rng.weighted(&[]), None);
        assert_eq!(rng.weighted(&[0.0, 0.0]), None);
    }

    #[test]
    fn below_covers_its_range_and_refuses_zero() {
        let mut rng = Rng::new(11);
        assert_eq!(rng.below(0), None);
        let mut seen = [false; 4];
        for _ in 0..256 {
            if let Some(index) = rng.below(4)
                && let Some(slot) = seen.get_mut(index)
            {
                *slot = true;
            }
        }
        assert!(seen.iter().all(|hit| *hit));
    }
}
