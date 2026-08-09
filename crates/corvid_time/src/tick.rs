//! The tick number, and the arithmetic a replay does on it.

use core::fmt;

/// One step of the simulation, counted from the opening.
///
/// A tick is an index rather than a moment. It says which step of the
/// simulation a state, an action, or a digest belongs to, and it means the same
/// thing on every peer replaying the same session -- which is what lets a
/// rollback name the tick it is rolling back to and a recorded trace name the
/// tick it disagrees on. The wall-clock time that tick happened to run at is
/// not recorded anywhere, because nothing deterministic may depend on it.
///
/// The field is public because a tick is a number and pretending otherwise
/// would cost every caller an accessor. The methods here exist for the
/// arithmetic that has an edge: all of it saturates.
///
/// ```
/// use corvid_time::Tick;
///
/// assert_eq!(Tick::ZERO.next(), Tick(1));
/// assert_eq!(Tick(100).since(Tick(60)), 40);
///
/// // A tick that ran before the one asked about is zero ticks ago, not a
/// // negative number of them.
/// assert_eq!(Tick(60).since(Tick(100)), 0);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(::serde::Serialize, ::serde::Deserialize),
    serde(transparent)
)]
pub struct Tick(
    /// How many ticks have run since the opening.
    pub u64,
);

impl Tick {
    /// The first tick of a session, before anything has been simulated.
    pub const ZERO: Self = Self(0);

    /// The tick after this one.
    ///
    /// Saturating, like every operation here. A wrapping counter would let
    /// `since` report a gap of nearly `u64::MAX` between two adjacent ticks and
    /// send a replay looking for a snapshot it will never find. At the fifteen
    /// ticks a second the simulation runs at, saturation is thirty-nine billion
    /// years away and the branch is only there to keep the panic out.
    #[must_use]
    #[inline]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    /// The tick before this one, or [`ZERO`](Self::ZERO) at the opening.
    #[must_use]
    #[inline]
    pub const fn prev(self) -> Self {
        Self(self.0.saturating_sub(1))
    }

    /// How many ticks have run since `earlier`, or zero if `earlier` is later.
    ///
    /// The saturation is the point: a log indexes by this, so an answer that
    /// went negative would have to be an `i64` that every caller then had to
    /// check. Ordering answers "which came first"; this answers "how far", and
    /// the two questions stay separate.
    #[must_use]
    #[inline]
    pub const fn since(self, earlier: Self) -> u64 {
        self.0.saturating_sub(earlier.0)
    }

    /// The tick `ticks` steps after this one.
    #[must_use]
    #[inline]
    pub const fn saturating_add(self, ticks: u64) -> Self {
        Self(self.0.saturating_add(ticks))
    }

    /// The tick `ticks` steps before this one, or [`ZERO`](Self::ZERO).
    #[must_use]
    #[inline]
    pub const fn saturating_sub(self, ticks: u64) -> Self {
        Self(self.0.saturating_sub(ticks))
    }
}

impl From<u64> for Tick {
    #[inline]
    fn from(ticks: u64) -> Self {
        Self(ticks)
    }
}

impl From<Tick> for u64 {
    #[inline]
    fn from(tick: Tick) -> Self {
        tick.0
    }
}

impl fmt::Display for Tick {
    /// Writes the number and nothing else, so a captured artefact can be named
    /// after the tick that produced it and sort the way a reader expects.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}
