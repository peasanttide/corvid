//! One machine's whole lockstep state.

mod exchange;
mod speak;
mod step;
mod transfer;

use alloc::vec::Vec;
use core::fmt;

use corvid_behavior::{PlayerId, State};
use corvid_replay::{Session, Snapshots};
use corvid_time::Tick;

use crate::{Budget, Frontier};

/// One machine's whole lockstep state.
///
/// It produces and consumes frames of bytes and carries none of them. A
/// `Transport` is the runtime's business, which is what lets this be driven
/// with no network in the process at all -- the tests here hand datagrams from
/// one peer to another by value.
///
/// # What a tick looks like from here
///
/// [`submit`](Self::submit) this machine's action for `now + delay`,
/// [`receive`](Self::receive) whatever arrived, [`advance`](Self::advance)
/// while the budget allows, and send [`outgoing`](Self::outgoing). A game
/// implements nothing.
pub struct Peer<S: State> {
    /// The session this peer is playing: the opening, the log, and the marks.
    pub session: Session<S>,
    /// The states it can restore from. A rollback lands on the newest of these
    /// at or before the corrected tick, so how many it holds decides what a
    /// rollback costs and not what it computes.
    pub snapshots: Snapshots<S>,
    /// How far every seat has been confirmed to.
    pub frontier: Frontier,
    /// How far ahead of that this peer will go.
    pub budget: Budget,
    /// Which seat this machine is.
    seat: PlayerId,
    /// The tick this peer's state is at.
    tick: Tick,
    /// The state at [`tick`](Self::tick).
    ///
    /// By value, not behind a handle. A session's `origin` is an [`alloc::sync::Arc`]
    /// because a runtime displays it, but nothing shares this one: a peer
    /// replaces it every tick and hands out clones, so a handle would buy a
    /// refcount and cost the ability to move a fresh state straight in.
    state: S,
    /// How deep the last rollback was.
    depth: u8,
    /// The tick this peer was on before a rollback deeper than its budget
    /// rewound it, and its own tick otherwise.
    resume: Tick,
    /// The newest tick this peer's marks have been compared against another
    /// peer's and agreed.
    agreed_marks: Tick,
    /// Whose mark was compared last, so that a report can name them.
    blamed: PlayerId,
    /// The row a tick is simulated against, kept so that one buffer serves
    /// every tick.
    row: Vec<S::Action>,
    /// The newest tick each seat has said it has every action for, which is the
    /// acknowledgement its datagrams carry.
    ///
    /// What it decides is how far back the window this peer sends reaches: a
    /// seat that has heard nothing for a second is a seat whose whole gap goes
    /// in the next packet. The minimum over the other seats is what
    /// [`outgoing`](Self::outgoing) uses, because one datagram goes to all of
    /// them and the one furthest behind is the one that needs the rows.
    ///
    /// [`None`] is a seat that has acknowledged nothing at all, which is not
    /// the same as one that has acknowledged the opening: the first would want
    /// the opening's own row sent again and the second would not.
    heard: Vec<Option<Tick>>,
    /// The newest tick this peer has ever simulated to, which is not
    /// [`tick`](Self::tick) while a rollback is being worked off.
    ///
    /// What it decides is which simulation of a tick is the first one, and that
    /// decides which of them may ask the runtime for anything --
    /// [`commands`](Self::commands) is where the argument lives.
    reached: Tick,
}

impl<S: State> Peer<S> {
    /// How many bytes of state [`new`](Self::new) lets the snapshot ring charge
    /// itself.
    ///
    /// Sixty-four mebibytes holds about sixty states of fifty thousand
    /// entities, which is ten times the deepest rollback the default
    /// [`Budget`] allows and leaves the rest for a slider. A machine with
    /// another number in mind builds the ring itself and hands it to
    /// [`with_snapshots`](Self::with_snapshots).
    pub const SNAPSHOT_BYTES: usize = 64 << 20;

    /// A peer at its session's opening.
    #[must_use]
    pub fn new(session: Session<S>, seat: PlayerId, budget: Budget) -> Self {
        Self::with_snapshots(session, seat, budget, Snapshots::new(Self::SNAPSHOT_BYTES))
    }

    /// The same, with a snapshot ring of its own.
    #[must_use]
    pub fn with_snapshots(
        session: Session<S>,
        seat: PlayerId,
        budget: Budget,
        snapshots: Snapshots<S>,
    ) -> Self {
        let mut frontier = Frontier::new(session.log.players());
        // A session that already knows somebody left -- one resumed from a save,
        // or one a state transfer handed over -- starts not waiting for them.
        // Retirement is derived from the roster rather than remembered beside
        // it, which is what makes two machines holding the same session hold
        // the same answer.
        for (seat, profile) in session.opening.roster.iter().enumerate() {
            if profile.left.is_some()
                && let Ok(seat) = u16::try_from(seat)
            {
                frontier.retire(PlayerId(seat));
            }
        }
        let seats = usize::from(frontier.seats());
        let tick = session.first();
        let state = S::clone(&session.opening.origin());
        Self {
            snapshots,
            frontier,
            budget,
            seat,
            tick,
            state,
            depth: 0,
            resume: tick,
            agreed_marks: tick,
            blamed: seat,
            row: Vec::new(),
            heard: alloc::vec![None::<Tick>; seats],
            reached: tick,
            session,
        }
    }

    /// Which seat this machine is.
    #[must_use]
    pub const fn seat(&self) -> PlayerId {
        self.seat
    }

    /// The tick this peer's state is at.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The state at [`tick`](Self::tick).
    #[must_use]
    pub const fn state(&self) -> &S {
        &self.state
    }

    /// How deep the last rollback was, for the overlay and the lab's graph.
    #[must_use]
    pub const fn depth(&self) -> u8 {
        self.depth
    }

    /// The newest tick this peer's marks have agreed with another peer's.
    #[must_use]
    pub const fn agreed_through(&self) -> Tick {
        self.agreed_marks
    }

    /// Whether this peer is behind where it wants to be.
    ///
    /// True while it is working off a rollback deeper than
    /// [`Budget::rollback`](crate::Budget::rollback), which it does one tick per
    /// [`advance`](Self::advance) rather than all at once.
    #[must_use]
    pub const fn stalled(&self) -> bool {
        self.tick.0 < self.resume.0
    }
}

impl<S: State> fmt::Debug for Peer<S> {
    /// The shape rather than the state. A peer holding fifty thousand entities
    /// prints them in the one place this gets called from, which is a failing
    /// assertion.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Peer")
            .field("seat", &self.seat)
            .field("tick", &self.tick)
            .field("depth", &self.depth)
            .field("budget", &self.budget)
            .field("frontier", &self.frontier)
            .field("snapshots", &self.snapshots)
            .finish_non_exhaustive()
    }
}
