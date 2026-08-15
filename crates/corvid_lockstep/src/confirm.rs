//! How far every seat has been confirmed to, and how far ahead of that a peer
//! is willing to go.

use alloc::vec::Vec;

use corvid_behavior::PlayerId;
use corvid_time::Tick;

/// How far every seat has been confirmed to.
///
/// One entry per seat, dense, and never moving backwards. A datagram that
/// arrives late cannot un-confirm a tick, because the only thing this records
/// is the newest tick a seat has spoken for.
///
/// ```
/// # use corvid_behavior::PlayerId;
/// # use corvid_lockstep::Frontier;
/// # use corvid_time::Tick;
/// let mut frontier = Frontier::new(3);
/// frontier.observe(PlayerId(0), Tick(10));
/// frontier.observe(PlayerId(1), Tick(12));
/// frontier.observe(PlayerId(2), Tick(9));
///
/// // Simulation below this is final and its snapshots can never go stale.
/// assert_eq!(frontier.agreed(), Tick(9));
///
/// // A reordered datagram arrives for a tick already passed.
/// frontier.observe(PlayerId(1), Tick(4));
/// assert_eq!(frontier.of(PlayerId(1)), Tick(12));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Frontier {
    /// The newest tick each seat has confirmed, and [`None`] for a seat that
    /// has never confirmed anything.
    ///
    /// The absence is a value of its own rather than the opening tick, because
    /// "confirmed the opening" and "has never spoken" predict differently: the
    /// first repeats what the seat did and the second falls back to
    /// `Action::default()`.
    seats: Vec<Option<Tick>>,
    /// Which seats have gone, and are therefore no longer waited for.
    retired: Vec<bool>,
}

impl Frontier {
    /// A frontier for `seats` seats, none of which has confirmed anything.
    #[must_use]
    pub fn new(seats: u16) -> Self {
        Self {
            seats: alloc::vec![None; usize::from(seats)],
            retired: alloc::vec![false; usize::from(seats)],
        }
    }

    /// How many seats it covers.
    #[must_use]
    pub fn seats(&self) -> u16 {
        u16::try_from(self.seats.len()).unwrap_or(u16::MAX)
    }

    /// The newest tick every seat has confirmed.
    ///
    /// Simulation below this is final and its snapshots can never go stale. A
    /// frontier with no seats answers the opening tick, because a session
    /// nobody is playing agrees about everything.
    #[must_use]
    pub fn agreed(&self) -> Tick {
        let live = self
            .seats
            .iter()
            .zip(self.retired.iter())
            .filter(|(_, retired)| !**retired)
            .map(|(seat, _)| seat.unwrap_or(Tick::ZERO))
            .min();
        // Every seat retired is a session with nobody left to disagree with, and
        // the answer that keeps such a peer playing rather than stalling for
        // ever is its own newest confirmation.
        live.unwrap_or_else(|| {
            self.seats
                .iter()
                .map(|seat| seat.unwrap_or(Tick::ZERO))
                .max()
                .unwrap_or(Tick::ZERO)
        })
    }

    /// Stops waiting for a seat, because the machine playing it has gone.
    ///
    /// [`agreed`](Self::agreed) skips a retired seat, so the peers still here
    /// carry on rather than stalling for ever against somebody who has closed
    /// their window; and prediction for it falls to `Action::default()` rather
    /// than repeating the last thing they did, which is what
    /// [`Presence::Dropped`](corvid_behavior::Presence) means.
    ///
    /// # This is derived rather than decided
    ///
    /// **Retiring a seat changes what this machine simulates**, so two machines
    /// that retired the same seat on different ticks would compute different
    /// states from there on -- which is a desync, and it would be this crate's
    /// fault.
    ///
    /// So nothing calls this on its own judgement. It is what
    /// [`Peer::depart`](crate::Peer::depart) does *after* writing the tick into
    /// [`Profile::left`](corvid_replay::Profile), and a peer built from a
    /// session whose roster already records a departure retires that seat as it
    /// opens. The tick is the session's; this is the local consequence of it,
    /// and two machines holding the same session hold the same one.
    pub fn retire(&mut self, seat: PlayerId) {
        if let Some(retired) = self.retired.get_mut(usize::from(seat.0)) {
            *retired = true;
        }
    }

    /// Whether this seat has gone.
    #[must_use]
    pub fn is_retired(&self, seat: PlayerId) -> bool {
        self.retired
            .get(usize::from(seat.0))
            .copied()
            .unwrap_or(false)
    }

    /// The newest tick one seat has confirmed, or the opening tick for a seat
    /// that has never confirmed anything.
    #[must_use]
    pub fn of(&self, seat: PlayerId) -> Tick {
        self.confirmed(seat).unwrap_or(Tick::ZERO)
    }

    /// The newest tick one seat has confirmed, and [`None`] for a seat that has
    /// never confirmed anything.
    #[must_use]
    pub fn confirmed(&self, seat: PlayerId) -> Option<Tick> {
        self.seats.get(usize::from(seat.0)).copied().flatten()
    }

    /// Whether a seat has ever confirmed anything.
    #[must_use]
    pub fn acted(&self, seat: PlayerId) -> bool {
        self.confirmed(seat).is_some()
    }

    /// Records that `seat` has confirmed through `through`.
    ///
    /// Never backwards, so a reordered datagram cannot un-confirm a tick. A
    /// seat this frontier does not cover is ignored.
    pub fn observe(&mut self, seat: PlayerId, through: Tick) {
        if let Some(slot) = self.seats.get_mut(usize::from(seat.0)) {
            *slot = Some(slot.map_or(through, |had| if had > through { had } else { through }));
        }
    }

    /// Which seats are being predicted at `at` -- the ones that have not
    /// confirmed it.
    pub fn predicted(&self, at: Tick) -> impl Iterator<Item = PlayerId> + '_ {
        self.seats
            .iter()
            .enumerate()
            .filter(move |(_, confirmed)| confirmed.is_none_or(|through| through < at))
            .filter_map(|(seat, _)| u16::try_from(seat).ok().map(PlayerId))
    }
}

/// How far ahead of [`Frontier::agreed`] a peer is willing to simulate.
///
/// ```
/// # use corvid_lockstep::Budget;
/// assert_eq!(Budget::DEFAULT.delay, 2);
/// assert_eq!(Budget::DEFAULT.rollback, 6);
/// assert_eq!(Budget::DEFAULT.ahead, 8);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Budget {
    /// Ticks of input delay: actions are submitted for `now + delay`, which is
    /// latency traded for fewer rollbacks.
    pub delay: u8,
    /// The most ticks that may ever be re-simulated at once. Past this the peer
    /// rewinds and works the rest off over the frames that follow -- a visible
    /// hitch is better than a missed frame budget.
    pub rollback: u8,
    /// The most ticks ahead of [`Frontier::agreed`] the peer will run. Past
    /// this it waits, because predicting further is predicting a decision.
    pub ahead: u8,
}

impl Budget {
    /// Two ticks of delay, six of rollback, eight ahead.
    ///
    /// Six rollback ticks over fifty thousand entities inside one 66
    /// millisecond tick is the bar the whole design is measured against.
    pub const DEFAULT: Self = Self {
        delay: 2,
        rollback: 6,
        ahead: 8,
    };

    /// A budget with the three numbers spelled out.
    #[must_use]
    pub const fn new(delay: u8, rollback: u8, ahead: u8) -> Self {
        Self {
            delay,
            rollback,
            ahead,
        }
    }

    /// The newest tick a peer at `tick` will record an action for.
    ///
    /// This is what makes a datagram naming a tick far in the future a refusal
    /// rather than a request for as much memory as the number says.
    #[must_use]
    pub fn horizon(&self, tick: Tick) -> Tick {
        tick.saturating_add(u64::from(self.delay).saturating_add(u64::from(self.ahead)))
    }
}

impl Default for Budget {
    /// [`DEFAULT`](Self::DEFAULT).
    fn default() -> Self {
        Self::DEFAULT
    }
}
