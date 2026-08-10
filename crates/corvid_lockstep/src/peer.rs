//! One machine's whole lockstep state.

use alloc::vec::Vec;
use core::fmt;

use corvid_behavior::{PlayerId, State};
use corvid_hash::{Digest, digest};
use corvid_replay::{Refused, Session, Snapshots, Unreachable};
use corvid_time::Tick;

use crate::{
    Advanced, Budget, Correction, Datagram, Desync, Frontier, Halt, Resync, Rolled, absorb,
    predict::{predict, row_at},
    rollback::step,
};

/// One machine's whole lockstep state.
///
/// It produces and consumes frames of bytes and carries none of them. A
/// `Transport` is the runtime's business, which is what lets this be driven
/// with no network in the process at all — the tests here hand datagrams from
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
    /// decides which of them may ask the runtime for anything —
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
        // A session that already knows somebody left — one resumed from a save,
        // or one a state transfer handed over — starts not waiting for them.
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
    /// [`Budget::rollback`], which it does one tick per
    /// [`advance`](Self::advance) rather than all at once.
    #[must_use]
    pub const fn stalled(&self) -> bool {
        self.tick.0 < self.resume.0
    }

    /// Submits this machine's action, for `now + delay`.
    ///
    /// Input delay is latency traded for fewer rollbacks: an action submitted
    /// two ticks ahead usually reaches the other peers before they simulate the
    /// tick it belongs to, and a prediction that was never needed is a rollback
    /// that never happens.
    ///
    /// # Errors
    ///
    /// [`Refused`], from the log, for a seat this session does not have or a
    /// tick this machine could not find the room for. [`Refused::Confirmed`] if
    /// this machine has already submitted a different action for that tick,
    /// which is this peer contradicting itself.
    ///
    /// # A rollback does not let a peer change its mind
    ///
    /// A correction can put [`tick`](Self::tick) back by several ticks, and the
    /// naive `now + delay` would then name a tick this machine has already
    /// spoken for — so a peer would submit a second, different action for it,
    /// send both, and be reported by everyone else as
    /// [`Halt::Contradiction`]. What this submits for instead is the first tick
    /// it has *not* spoken for, which is never behind `now + delay` and is
    /// sometimes ahead of it. The cost is that the ticks a deep rollback
    /// replayed keep the actions they were played with, which is the correct
    /// answer: they are what this player did, and the other machines have
    /// already simulated them.
    pub fn submit(&mut self, action: S::Action) -> Result<Tick, Refused> {
        let at = self.tick.saturating_add(u64::from(self.budget.delay));
        // Already spoken for. A peer that is stalled, or that a correction has
        // put back a few ticks, reaches this every tick until its own tick
        // catches up again — and the action it is holding down now is not an
        // action for a tick it announced two seconds ago. Dropping it is the
        // only answer that keeps one story on the wire.
        if self
            .frontier
            .confirmed(self.seat)
            .is_some_and(|spoken| spoken >= at)
        {
            drop(action);
            return Ok(self.frontier.of(self.seat));
        }
        self.session.log.extend_to(at)?;
        self.session.log.set(at, self.seat, action)?;
        self.frontier.observe(self.seat, at);
        Ok(at)
    }

    /// What to send this tick.
    ///
    /// The newest [`WINDOW`](crate::WINDOW) rows of this seat's own actions,
    /// and one digest. The digest is for the newest tick this peer's state is
    /// *final* at — its own tick or [`Frontier::agreed`], whichever is older —
    /// because a mark taken over a prediction is not a fact and comparing one
    /// would report a desync every time a packet was late.
    #[must_use]
    pub fn outgoing(&self) -> Datagram<S::Action> {
        // What this seat has actually submitted, and never a tick past it. The
        // window is read out of the log, and a row this peer has not written is
        // `Action::default()` — so a head taken from `now + delay` alone would
        // put "idle" on the wire for a tick this machine has not decided yet,
        // every other peer would confirm it, and the real action arriving a
        // tick later would be reported as this peer contradicting itself. It is
        // reachable from an ordinary call order: `advance` moves `tick`, so
        // sending after simulating overshoots by exactly one tick.
        let want = self.tick.saturating_add(u64::from(self.budget.delay));
        let head = self.frontier.confirmed(self.seat).map_or_else(
            || self.session.first(),
            |spoken| {
                if want < spoken { want } else { spoken }
            },
        );
        let agreed = self.frontier.agreed();
        let marked = if self.tick < agreed {
            self.tick
        } else {
            agreed
        };
        let mark = self.session.marks.get(marked).unwrap_or_default();
        Datagram::build(
            &self.session.log,
            self.seat,
            head,
            self.acked(),
            self.heard_through(),
            marked,
            mark,
        )
    }

    /// The newest tick this peer has every seat's real action for, and [`None`]
    /// while any seat has said nothing at all.
    ///
    /// [`Frontier::agreed`] alone would answer the opening tick for a peer that
    /// has heard from nobody, which is a claim rather than a report — this is
    /// the same number with the claim removed.
    fn heard_through(&self) -> Option<Tick> {
        (0..self.frontier.seats())
            .all(|seat| self.frontier.acted(PlayerId(seat)))
            .then(|| self.frontier.agreed())
    }

    /// The newest tick every *other* seat has said it has everything through.
    ///
    /// The minimum, because one datagram goes to all of them: the seat furthest
    /// behind is the one whose missing rows have to be in it. A session with
    /// nobody else in it answers this peer's own frontier, which makes the
    /// window the minimum [`WINDOW`](crate::WINDOW) rows.
    fn acked(&self) -> Option<Tick> {
        self.heard
            .iter()
            .enumerate()
            .filter(|(seat, _)| *seat != usize::from(self.seat.0))
            .map(|(_, heard)| *heard)
            // `None` is a seat that has acknowledged nothing, and `Option`'s own
            // order puts it below every tick — so a seat still catching up
            // pulls the window all the way back, which is exactly what it
            // needs.
            .min()
            .unwrap_or_default()
    }

    /// Folds in what arrived, rolling back if it must.
    ///
    /// # Errors
    ///
    /// [`Halt::Refused`] for a datagram naming a tick past
    /// [`Budget::horizon`], which is the denial-of-service arm: a tick number
    /// is the one thing in a session that arrives from somewhere else, and a
    /// log that grew to whatever it said would be a request for as much memory
    /// as the number. [`Halt::Contradiction`] for a peer that has sent two
    /// different actions for one tick, and [`Halt::Desync`] for one whose mark
    /// disagrees with this peer's own.
    /// # No sink
    ///
    /// A datagram never reaches one. All this can do is roll back, and a tick
    /// replayed to work off a rollback is this machine recomputing a state
    /// rather than the game asking for anything a second time — so there is
    /// nothing here for a sink to receive.
    pub fn receive(&mut self, datagram: &Datagram<S::Action>) -> Result<Rolled, Halt> {
        // How far this machine is willing to make room for, and never further:
        // a tick number is the one thing in a session that arrives from
        // somewhere else, and a log that grew to whatever it said would be a
        // request for as much memory as the number.
        //
        // A datagram past it is **clamped rather than refused**. A peer that
        // started a second later than its opponent receives windows whose
        // newest rows are past its horizon on every packet, and refusing them
        // would end the session before it began; the rows it can use are folded
        // in, the rest stay in the sender's window until this machine
        // acknowledges them, and it catches up. The same clamp is what stops a
        // stranger ending a game by sending one large number.
        let horizon = self.budget.horizon(self.tick);
        let head = datagram.head();
        // Entirely about ticks this session no longer holds. A bounded run
        // forgets its far past and a `resync` forgets everything before the
        // state it adopted, so a datagram that was in flight across either of
        // those arrives naming rows that are gone — which is ordinary, and is
        // not something to stop for.
        if head < self.session.first() {
            return Ok(Rolled::default());
        }
        let ceiling = if head < horizon { head } else { horizon };
        self.session.log.extend_to(ceiling)?;

        // What the sender says it has, which is what decides how far back this
        // peer's own window reaches from now on. Never backwards: a reordered
        // datagram carries an older acknowledgement and un-acknowledging a row
        // would put it back in every packet.
        if let Some(heard) = self.heard.get_mut(usize::from(datagram.seat.0))
            && *heard < datagram.heard
        {
            *heard = datagram.heard;
        }

        let correction = absorb(&mut self.session.log, &mut self.frontier, datagram)?;
        let rolled = match correction {
            Correction::Agreed | Correction::Duplicate => Rolled::default(),
            Correction::Contradiction { at } => {
                return Err(Halt::Contradiction {
                    peer: datagram.seat,
                    at,
                });
            }
            Correction::Mispredicted { at } => self.roll_back(at)?,
        };

        self.check_mark(datagram.seat, datagram.marked, datagram.mark)?;
        Ok(rolled)
    }

    /// Simulates one tick forward, predicting whatever has not arrived.
    ///
    /// # Errors
    ///
    /// [`Halt::Refused`] if the log could not be grown to the tick being
    /// simulated.
    /// # Only a tick's first simulation reaches the sink
    ///
    /// A rollback re-simulates ticks this peer has already been through, and a
    /// [`save`](corvid_behavior::Command::save) is a file rather than a value —
    /// so a peer on a link that mispredicts every second tick would write one
    /// save per correction if the re-simulation reached `command` too. It does
    /// not: a replayed tick is handed a
    /// [`Discard`](corvid_behavior::Discard) instead.
    ///
    /// What that rule costs is worth stating rather than leaving to be found. A
    /// networked game reaches the same command stream a single-seat one does
    /// **for the ticks that were never corrected**, and that is the honest form
    /// of the claim: a command from a tick whose prediction turned out wrong
    /// was asked for by a state that never happened, and nothing here can unsay
    /// it. A game whose requests must survive that puts the request in its
    /// `State` and lets the client read it out of a confirmed tick.
    pub fn advance(
        &mut self,
        command: &mut impl corvid_behavior::Command,
    ) -> Result<Advanced, Halt> {
        let ceiling = self
            .frontier
            .agreed()
            .saturating_add(u64::from(self.budget.ahead));
        if self.tick >= ceiling {
            return Ok(Advanced {
                tick: self.tick,
                predicted_seats: 0,
                stalled: true,
            });
        }

        let predicted = predict(&mut self.session.log, &self.frontier, self.tick)?;
        self.simulate_one(command);
        if self.tick > self.resume {
            self.resume = self.tick;
        }
        Ok(Advanced {
            tick: self.tick,
            predicted_seats: predicted.seats,
            stalled: self.stalled(),
        })
    }

    /// Compares an arrived mark against this peer's own trace.
    ///
    /// A tick this peer has no mark for says nothing, which is what a peer that
    /// has not got there yet honestly knows.
    ///
    /// # Errors
    ///
    /// [`Halt::Desync`] when the two digests differ, naming the tick they
    /// differ at rather than the tick the mark arrived on.
    pub fn compare(&self, seat: PlayerId, at: Tick, mark: Digest) -> Result<(), Halt> {
        let Some(local) = self.session.marks.get(at) else {
            return Ok(());
        };
        if local == mark {
            return Ok(());
        }
        Err(Halt::Desync(Desync {
            at,
            peer: seat,
            agreed_through: self.agreed_marks,
            local,
            remote: mark,
            fields: Vec::new(),
            first_divergent: None,
        }))
    }

    /// Records that a seat has left, on a tick every machine agrees on.
    ///
    /// A runtime calls this when its transport reports that a peer has gone.
    /// Without it, a player who closes their window leaves everybody else
    /// stalled against a frontier that will never move again — the session does
    /// not desync, it simply stops, which is the worse of the two failures
    /// because nothing reports it.
    ///
    /// # Why this takes a tick, and why that makes it safe
    ///
    /// A departure changes what every machine simulates: from `at` the seat is
    /// [`Presence::Dropped`](corvid_behavior::Presence) and submits
    /// `Action::default()` for ever. So two machines that decided it on
    /// different ticks would compute different states — which is a desync, and
    /// it would be this crate's fault.
    ///
    /// What makes it safe is that the tick is part of the *session* rather than
    /// a decision each machine makes for itself: it is written into
    /// [`Profile::left`](corvid_replay::Profile), it is what a save carries and
    /// a replay reproduces, and **the earliest one wins**. Two runtimes that
    /// propose different ticks for the same seat both end up applying the lower
    /// of the two, in either order, because this refuses to move a departure
    /// later. That is the whole of the agreement protocol, and it needs no
    /// round trip: a proposal is idempotent and commutative, so a runtime can
    /// broadcast what it believes and fold in what it hears.
    ///
    /// The rollback is the other half. A departure at a tick this peer has
    /// already simulated past invalidates every state after it, so this rewinds
    /// to `at` and re-simulates — exactly as a late action does, and through the
    /// same budget.
    ///
    /// # It is one-way
    ///
    /// **Nothing un-departs a seat.** A machine that comes back is a machine
    /// with a state nobody else's session agrees with, and what makes it
    /// playable again is a whole state transferred to it and
    /// [`adopt`](Self::adopt)ed — at which point it is holding this session's
    /// roster, departures and all. Moving a `left` tick later, or clearing it,
    /// would be this machine editing history every other machine has already
    /// simulated.
    ///
    /// # No sink
    ///
    /// Like [`receive`](Self::receive): all this can do is roll back, and a
    /// replayed tick asks the runtime for nothing.
    ///
    /// # Errors
    ///
    /// [`Halt::Unreachable`] for a tick before the session's opening, and
    /// whatever a rollback to `at` reports.
    pub fn depart(&mut self, seat: PlayerId, at: Tick) -> Result<Rolled, Halt> {
        let first = self.session.first();
        if at < first {
            return Err(Unreachable::Before { to: at, first }.into());
        }
        let Some(profile) = self.session.opening.roster.get_mut(usize::from(seat.0)) else {
            // A seat this session does not have. Nothing to record, and nothing
            // to report either: a transport can name a peer no roster seats.
            return Ok(Rolled::default());
        };
        // The earliest wins, and a departure already at or before this one is
        // the whole answer.
        if profile.left.is_some_and(|already| already <= at) {
            return Ok(Rolled::default());
        }
        profile.left = Some(at);
        self.frontier.retire(seat);

        // Every state from `at` on was computed with this seat present, so they
        // are all wrong now. The correction lands on `at` for the reason every
        // other correction does: the state *at* a tick is what the rows before
        // it produced, and what changed is the roster the tick at `at` is
        // simulated with.
        self.roll_back(at)
    }

    /// The tick a seat left on, if it has.
    #[must_use]
    pub fn departed(&self, seat: PlayerId) -> Option<Tick> {
        self.session
            .opening
            .roster
            .get(usize::from(seat.0))
            .and_then(|profile| profile.left)
    }

    /// Asks for a whole state, which is what a build without `dev` does instead
    /// of bisecting.
    #[must_use]
    pub const fn resync_request(&self, at: Tick) -> Resync {
        Resync {
            seat: self.seat,
            at,
            agreed_through: self.agreed_marks,
        }
    }

    /// Adopts a state that arrived over a reliable channel.
    ///
    /// The snapshot ring is emptied rather than corrected: two peers' states
    /// share no history this machine can compare, so nothing it is holding can
    /// be trusted to be about the session that is resuming.
    ///
    /// Nothing here simulates, so no scratch passes through.
    ///
    /// A state for a tick this peer has already reached replaces what it
    /// computed there, and the trace behind it is kept — that is a machine
    /// being told it was wrong about a stretch it played. For a state from
    /// *ahead* of this peer, which is what rescues one that can no longer catch
    /// up from actions, see [`resync`](Self::resync).
    ///
    /// # Errors
    ///
    /// [`Halt::Unreachable`] for a tick before the session's opening or after
    /// the one this peer has reached.
    pub fn adopt(&mut self, at: Tick, state: S) -> Result<(), Halt> {
        let first = self.session.first();
        if at < first {
            return Err(Unreachable::Before { to: at, first }.into());
        }

        if at > self.tick {
            return Err(Unreachable::After {
                to: at,
                last: self.tick,
            }
            .into());
        }

        self.snapshots.clear();
        self.state = state;
        self.tick = at;
        self.resume = at;
        // The ticks after `at` are about to be simulated from a state that
        // arrived from another machine, so none of them has been simulated
        // *from this state* before and every one of them is a first time. A
        // high-water mark left where it was would silence the commands of every
        // tick between here and where this peer had got to.
        self.reached = at;
        self.depth = 0;
        self.session.marks.truncate_from(at);
        self.session.marks.push(digest(&self.state));
        self.agreed_marks = at;
        self.snapshots.keep(&self.session.log, at, &self.state);
        Ok(())
    }

    /// Reopens the session at `at` on `state`, forgetting everything before it.
    ///
    /// **This is what ends a stall no window of actions can end.** A peer whose
    /// link was down for longer than [`CATCHUP`](crate::CATCHUP) rows is
    /// missing actions nobody still sends: it is not behind, it is *stuck*, and
    /// no amount of waiting fixes it. What does is somebody's whole state, and
    /// what this does with one is refuse to pretend about the gap — the rows
    /// and marks before `at` are dropped, exactly as a bounded run drops its
    /// far past, and the session begins again there.
    ///
    /// The frontier is rebuilt with it, which is the half that is easy to miss:
    /// a peer that kept waiting for the rows it was waiting for before would
    /// adopt a state and stall again immediately, for the same reason.
    ///
    /// # Both ends do this
    ///
    /// The machine that *sends* a state calls this too, with its own. Otherwise
    /// it goes on waiting for rows the rescued machine will never send — they
    /// are older than the tick it just restarted at — and the session ends with
    /// one peer playing and one peer stuck, which is the failure it was trying
    /// to fix wearing the other hat.
    ///
    /// # Errors
    ///
    /// [`Halt::Unreachable`] for a tick before the session's opening, and
    /// [`Halt::Refused`] if the log could not be grown to reach it.
    pub fn resync(&mut self, at: Tick, state: S) -> Result<(), Halt> {
        let first = self.session.first();
        if at < first {
            return Err(Unreachable::Before { to: at, first }.into());
        }

        self.session.log.extend_to(at)?;
        let origin = alloc::sync::Arc::new(S::clone(&state));
        // The log was grown to `at` a line ago and `at` is at or after the
        // opening, so neither refusal this can answer is reachable — and it is
        // reported rather than ignored, so that it stays unreachable if either
        // of those changes.
        if self.session.forget_before(at, origin).is_err() {
            return Err(Unreachable::After {
                to: at,
                last: self.session.last(),
            }
            .into());
        }

        self.frontier = Frontier::new(self.session.log.players());
        for (seat, profile) in self.session.opening.roster.iter().enumerate() {
            if profile.left.is_some()
                && let Ok(seat) = u16::try_from(seat)
            {
                self.frontier.retire(PlayerId(seat));
            }
        }
        self.heard.iter_mut().for_each(|heard| *heard = None);

        self.snapshots.clear();
        self.state = state;
        self.tick = at;
        self.resume = at;
        self.reached = at;
        self.depth = 0;
        self.session.marks.truncate_from(at);
        self.session.marks.push(digest(&self.state));
        self.agreed_marks = at;
        self.snapshots.keep(&self.session.log, at, &self.state);
        Ok(())
    }

    /// The newest state this peer holds at or before `at`, and the tick it is
    /// the state at.
    ///
    /// The opening when the ring holds nothing usable, because the opening is
    /// always somewhere to start from.
    ///
    /// # Errors
    ///
    /// [`Halt::Unreachable`] for a tick before the session's opening.
    pub fn restore(&self, at: Tick) -> Result<(Tick, S), Halt> {
        let first = self.session.first();
        if at < first {
            return Err(Unreachable::Before { to: at, first }.into());
        }
        // The opening's origin is resolved into a handle before the borrow,
        // because `origin()` answers an owned `Arc` — a fresh session's is
        // built from `S::default()` rather than held anywhere — and a
        // reference into it would not outlive the expression.
        Ok(match self.snapshots.nearest(&self.session.log, at) {
            Some((tick, state)) => (tick, state.clone()),
            None => (first, S::clone(&self.session.opening.origin())),
        })
    }

    /// The rule, stated once so that no call site has to restate it.
    ///
    /// The state *at* `at` is the result of simulating the rows *before* `at`,
    /// so a correction to the row at `at` does not invalidate it: the ring is
    /// told to discard from `at.next()` and the snapshot at `at` is what the
    /// re-simulation starts from. Passing `at` would not be the cautious
    /// version of that — forward play keeps the state at `S` before row `S` is
    /// written, so every entry the ring ever holds would go and every rollback
    /// would replay from the opening.
    fn roll_back(&mut self, at: Tick) -> Result<Rolled, Halt> {
        let was = self.tick;
        if at >= was {
            return Ok(Rolled {
                from: at,
                to: was,
                ticks: 0,
            });
        }

        self.snapshots.discard_from(at.next());

        let ceiling = at.saturating_add(u64::from(self.budget.rollback));
        let target = if was > ceiling { ceiling } else { was };

        let (from, restored) = self.restore(at)?;
        self.state = restored;
        self.tick = from;
        self.session.marks.truncate_from(from.next());

        while self.tick < target {
            self.simulate_one(&mut corvid_behavior::Discard::new());
        }

        if self.resume < was {
            self.resume = was;
        }
        self.depth = u8::try_from(was.since(at)).unwrap_or(u8::MAX);
        Ok(Rolled {
            from: at,
            to: self.tick,
            ticks: u8::try_from(self.tick.since(at)).unwrap_or(u8::MAX),
        })
    }

    /// One tick forward from wherever this peer is, against the row prediction
    /// makes.
    fn simulate_one(&mut self, command: &mut impl corvid_behavior::Command) {
        row_at(&self.session.log, &self.frontier, self.tick, &mut self.row);
        // Whether this is the first time this tick has been simulated, read
        // before the tick moves. `reached` is the high-water mark rather than
        // `tick`, because a rollback puts `tick` back and the ticks it replays
        // are ticks this peer has already been through.
        let fresh = self.tick >= self.reached;
        // The rule, as a choice of sink rather than as a `Vec` filtered after
        // the fact: a tick simulated for the first time may ask the runtime for
        // things, and a tick being replayed to work off a rollback may not.
        // A tick that asked to quit, to save, or to rumble a pad asked once.
        self.state = if fresh {
            step::<S>(&self.session, &self.state, self.tick, &self.row, command)
        } else {
            step::<S>(
                &self.session,
                &self.state,
                self.tick,
                &self.row,
                &mut corvid_behavior::Discard::new(),
            )
        };
        self.tick = self.tick.next();
        if self.tick > self.reached {
            self.reached = self.tick;
        }
        self.session.marks.truncate_from(self.tick);
        self.session.marks.push(digest(&self.state));
        self.snapshots
            .keep(&self.session.log, self.tick, &self.state);
    }

    /// Compares a mark that arrived, when there is anything final to compare it
    /// against.
    ///
    /// A mark for a tick past [`Frontier::agreed`] is about a state one of the
    /// two peers predicted part of, so a disagreement there is a packet in
    /// flight rather than a divergence. The marks that matter arrive a moment
    /// later, for ticks both peers have confirmed.
    fn check_mark(&mut self, seat: PlayerId, at: Tick, mark: Digest) -> Result<(), Halt> {
        if at > self.frontier.agreed() {
            return Ok(());
        }
        self.blamed = seat;
        self.compare(seat, at, mark)?;
        if self.session.marks.get(at).is_some() && at > self.agreed_marks {
            self.agreed_marks = at;
        }
        Ok(())
    }

    /// A report about this peer, for the bisector to fill in.
    #[cfg(feature = "dev")]
    pub(crate) fn desync_at(
        &self,
        at: Tick,
        fields: Vec<crate::FieldReport>,
        first_divergent: Option<crate::Where>,
    ) -> Desync {
        let local = self.session.marks.get(at).unwrap_or_default();
        Desync {
            at,
            peer: self.blamed,
            agreed_through: self.agreed_marks,
            local,
            remote: local,
            fields,
            first_divergent,
        }
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
