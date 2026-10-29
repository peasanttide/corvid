//! One machine's whole lockstep state.

mod exchange;
mod held;
mod keep;
mod speak;
mod step;
mod transfer;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::fmt;

use corvid_behavior::{LevelEdit, PlayerId, SaveSlot, State};
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
    /// The seats this machine plays besides its own: a bot's, submitted
    /// with [`submit_for`](Self::submit_for).
    pub extra: Vec<PlayerId>,
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
    /// What the ticks not yet final asked the runtime for, by tick: the
    /// newest simulation of each, handed over once every seat has confirmed
    /// its row (`held.rs` has the argument).
    held: BTreeMap<Tick, held::Held<LevelEdit<S>>>,
    /// The ticks before this one are final and what they asked for has been
    /// handed over; a re-simulation of one of them asks nothing again.
    told: Tick,
    /// The saves final ticks asked for and the runtime has not written yet,
    /// each with the tick that asked: what a save holds is the state that
    /// tick produced, and the request alone does not say which tick that was
    /// (`keep.rs`).
    saved: Vec<(Tick, SaveSlot)>,
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
            extra: Vec::new(),
            heard: alloc::vec![None::<Tick>; seats],
            held: BTreeMap::new(),
            told: tick,
            saved: Vec::new(),
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
