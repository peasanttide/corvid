//! How many ticks, as opposed to which one.

/// A count of ticks.
///
/// A count and a point in time are different things: `Tick(30)` is the
/// thirty-first tick of a session and `Ticks(30)` is thirty of them, from
/// wherever the counting started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(::serde::Serialize, ::serde::Deserialize),
    serde(transparent)
)]
pub struct Ticks(pub u64);

impl Ticks {
    /// No ticks at all.
    pub const NONE: Self = Self(0);

    /// The tick `self` ticks after `from`, saturating at the end of the range.
    #[must_use]
    #[inline]
    pub const fn after(self, from: crate::Tick) -> crate::Tick {
        crate::Tick(from.0.saturating_add(self.0))
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::panic,
        clippy::unwrap_used,
        reason = "a failed assertion in a test is a failed test, which is what a test is for"
    )]

    use super::Ticks;
    use crate::Tick;

    #[test]
    fn a_count_of_ticks_lands_that_far_past_where_it_started() {
        assert_eq!(Ticks(10).after(Tick(5)), Tick(15));
        assert_eq!(Ticks::NONE.after(Tick(5)), Tick(5));
    }

    #[test]
    fn a_count_that_would_run_off_the_end_stops_at_it() {
        assert_eq!(Ticks(u64::MAX).after(Tick(1)), Tick(u64::MAX));
    }
}
