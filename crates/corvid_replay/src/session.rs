//! Everything needed to reproduce a session, and the two functions that write
//! it down and read it back.
//!
//! What it starts from is [`opening`](crate::opening), how it can refuse to
//! load is [`replay_error`](crate::replay_error), and the hand-written
//! encodings for both are [`encode`](crate::encode) -- apart because a reader
//! following what a session *is* is not the reader checking how it is written
//! down.

use alloc::{sync::Arc, vec::Vec};

use corvid_behavior::State;
use corvid_hash::Digest;
use corvid_time::Tick;

use crate::opening::Opening;
use crate::replay_error::{Forget, Load, Shape};
use crate::{ActionLog, HashTrace};

/// Everything needed to reproduce a session bit for bit.
///
/// The log is the game. The state at any tick is a function of the opening and
/// the actions up to it, which is what makes save, load, replay, rollback and
/// time-walk one operation -- see [`seek`](Self::seek).
///
/// The three fields are public because every one of them is read and written by
/// a layer this crate does not contain: a lockstep transport writes actions
/// into the log, a desync check reads the marks, a dev console reads the
/// opening. What that costs is that the three can be put out of step by hand --
/// a log whose first tick is not the opening's, a roster narrower than the
/// log's rows. [`load`](Self::load) refuses a capture like that with a named
/// error, and [`check`](Self::check) is that same refusal as a call, so that
/// anything assembling a session field by field can make it. Nothing can force
/// the call: a `pub` field has no assignment hook, and [`seek`](Self::seek) says
/// what it does with a session nobody checked.
pub struct Session<S: State> {
    /// What the session started from.
    pub opening: Opening<S>,
    /// One action per seat per tick, dense. This is the game.
    pub log: ActionLog<S::Action>,
    /// One digest per tick.
    pub marks: HashTrace,
}

/// A session is what an opening becomes, and this is that becoming.
///
/// The fallible half of the pair the workspace's conventions ask for: a total
/// conversion is a [`From`] and one that can refuse is a [`TryFrom`] with an
/// error type that says what it refused. [`Session::new`](crate::Session::new) is the same call
/// under the name a constructor is looked for by.
impl<S: State> TryFrom<Opening<S>> for Session<S> {
    type Error = Shape;

    /// # Errors
    ///
    /// [`Shape::Roster`](crate::Shape::Roster), for an opening whose roster is wider than a
    /// [`PlayerId`](corvid_behavior::PlayerId) can address, which is the only way this can fail.
    fn try_from(opening: Opening<S>) -> Result<Self, Shape> {
        let Some(seats) = opening.seats() else {
            return Err(Shape::Roster {
                seats: opening.roster.len(),
            });
        };
        let mut marks = HashTrace::new(opening.first);
        marks.push(opening.mark());
        Ok(Self {
            log: ActionLog::new(opening.first, seats),
            marks,
            opening,
        })
    }
}

impl<S: State> Session<S> {
    /// A session at its opening: an empty log and an empty trace, both
    /// positioned and sized from the opening.
    ///
    /// The first mark is pushed here, because the state at
    /// [`Opening::first`](Opening::first) is a state the session has and every
    /// other mark is about a state a tick produced.
    ///
    /// # Errors
    ///
    /// [`Shape::Roster`](crate::Shape::Roster), for an opening whose roster is wider than a
    /// [`PlayerId`](corvid_behavior::PlayerId) can address. That is the only way this can fail, and it
    /// fails rather than narrowing: a log is as wide as [`Opening::seats`], so
    /// a roster of seventy thousand would get 65 535 columns and leave 4 465
    /// seats with nowhere to put an action. A session with more seats than its
    /// log can address is not a session -- [`check`](Self::check) refuses
    /// exactly that capture -- and a constructor that returned one would be
    /// handing back something it already knows is broken.
    pub fn new(opening: Opening<S>) -> Result<Self, Shape> {
        Self::try_from(opening)
    }

    /// The first tick of the session.
    #[must_use]
    pub const fn first(&self) -> Tick {
        self.opening.first
    }

    /// The latest tick [`seek`](Self::seek) can reach.
    #[must_use]
    pub fn last(&self) -> Tick {
        self.opening.first.saturating_add(self.log.ticks())
    }

    /// Forgets everything before `tick`, reopening the session there with
    /// `origin` as the state at `tick`, and hands back the state it used to open
    /// at.
    ///
    /// A session that is being played forward accumulates a row of actions and a
    /// mark per tick and reads neither again, so something has to be able to let
    /// go of the far past. This is that something, and it is one call rather
    /// than three because the three parts have to move together: a log that
    /// forgot its first rows while the opening stayed where it was would be a
    /// session [`check`](Self::check) refuses and [`seek`](Self::seek) would
    /// replay from a state that is no longer at the tick it thinks it is.
    ///
    /// [`last`](Self::last) does not move and neither does the state at any tick
    /// still covered. What changes is how far back the session reaches: a seek
    /// before `tick` is [`Unreachable::Before`](crate::Unreachable::Before) from
    /// here on, and the marks and actions before it are gone. Save, replay,
    /// rollback and time-walk all still work, over the ticks that are left.
    ///
    /// # What this cannot check, and the caller therefore owes
    ///
    /// **That `origin` is the state at `tick`.** Nothing here can recompute it --
    /// that is what a seek is for, and a seek from a session that has just
    /// forgotten the rows in question is exactly the thing that is no longer
    /// available. A caller that hands over a state from some other tick has
    /// built a session that replays to a game that never happened, and every
    /// digest after it agrees with itself.
    ///
    /// **That the snapshot ring is told.** An entry kept at `tick` is keyed to a
    /// generation this call resets;
    /// [`ActionLog::forget_before`](crate::ActionLog::forget_before) is exact
    /// about which direction that fails in. Anything holding a
    /// [`Snapshots`](crate::Snapshots) alongside this wants
    /// [`discard_from`](crate::Snapshots::discard_from) at `tick`, which pops
    /// from the newest end and so covers exactly that entry and the ones after
    /// it. An entry kept *before* `tick` is keyed to rows that no longer exist,
    /// which `discard_from` cannot reach and which
    /// [`nearest`](crate::Snapshots::nearest) therefore refuses on its own: the
    /// log's [`first`](crate::ActionLog::first) is that search's floor. Nothing
    /// a caller can do about that half, so nothing is owed for it.
    ///
    /// # What this costs, now that an origin is a handle
    ///
    /// Nothing but the swap. A runtime calls this on a schedule with the state
    /// it is currently displaying, and that state is already an
    /// [`Arc`] -- it is the one it hands to every frame -- so the argument is a
    /// refcount bump rather than a copy of a whole simulation state. The old
    /// origin comes back as the handle the session was holding, which is the
    /// only sense in which it is "the" state at the old first tick: a snapshot
    /// ring, a frame still on screen or a peer's rollback buffer may be holding
    /// the same value, and this call says nothing about whether the caller has
    /// the last handle to it.
    ///
    /// It comes back rather than being dropped here because letting go of it is
    /// the caller's to decide. A runtime that still wants the old origin -- to
    /// compare against, to write out, to keep one more displayed frame -- would
    /// have no way of asking for it once this returned, and a runtime that does
    /// not want it drops it in one line.
    ///
    /// # Errors
    ///
    /// [`Forget::Early`] for a tick before the opening, which has nothing before
    /// it to forget, and [`Forget::Beyond`] for one past
    /// [`last`](Self::last), where the session has no state to be told about.
    /// The handle passed in is dropped in both cases, because a refused call
    /// leaves the session with nowhere to put it.
    pub fn forget_before(&mut self, tick: Tick, origin: Arc<S>) -> Result<Arc<S>, Forget> {
        let first = self.opening.first;
        if tick < first {
            return Err(Forget::Early { tick, first });
        }
        let last = self.last();
        if tick > last {
            return Err(Forget::Beyond { tick, last });
        }
        self.log.forget_before(tick);
        self.marks.forget_before(tick);
        self.opening.first = tick;
        // The old origin resolved: a session whose opening carried none was
        // opening on `S::default()`, and that is the state being replaced.
        let was = self.opening.origin();
        self.opening.origin = Some(origin);
        Ok(was)
    }

    /// Writes the session down, in `corvid_wire`.
    ///
    /// # Errors
    ///
    /// Whatever the encoder reports. A `Session` whose parts all serialize
    /// cannot fail here; a game whose types cannot be written down compactly
    /// fails at the first one that cannot, and
    /// [`round_trip_is_faithful`](corvid_wire::round_trip_is_faithful) is
    /// where that is worth finding out.
    pub fn save(&self) -> Result<Vec<u8>, corvid_wire::Error> {
        corvid_wire::encode(self)
    }

    /// Reads a session back, and refuses one this build cannot replay.
    ///
    /// `schema` is the digest of the *running* build's type schema. A capture
    /// records the digest of the build that wrote it, and the two are compared
    /// before anything is replayed, so a capture from an incompatible build is
    /// [`Load::Schema`] rather than a divergence a hundred ticks in. What that
    /// comparison can and cannot tell apart is [`Schema`](crate::Schema)'s
    /// documentation and is narrower than the name suggests.
    ///
    /// The shape of the capture is [`check`](Self::check)ed too, so a capture
    /// whose own parts disagree about the session they describe is
    /// [`Load::Shape`] rather than a replay that reads the wrong seat's action.
    ///
    /// # Errors
    ///
    /// [`Load::Bytes`] if the bytes are not a session, [`Load::Schema`] if the
    /// capture was recorded by a build that describes itself differently, and
    /// [`Load::Shape`] if the capture's own parts disagree about the session.
    pub fn load(bytes: &[u8], schema: Digest) -> Result<Self, Load> {
        let session: Self = corvid_wire::decode(bytes).map_err(Load::Bytes)?;
        if session.opening.schema != schema {
            return Err(Load::Schema {
                recorded: session.opening.schema,
                running: schema,
            });
        }
        session.check().map_err(Load::Shape)?;
        Ok(session)
    }

    /// Reports the first way this session's parts disagree about the session
    /// they describe, if they do.
    ///
    /// # The six ways, and why they are one call
    ///
    /// A session can be internally inconsistent while every individual field
    /// decodes perfectly: the log can start at a different tick than the opening
    /// or the trace can, the roster can name more seats than a [`PlayerId`](corvid_behavior::PlayerId) can
    /// address, the log's rows can be a different width than the roster, the
    /// log's entries can stop partway through a row, and the log's confirmation
    /// bitmap can be a different length than its actions need.
    ///
    /// The last two are the ones that do not announce themselves. A short
    /// bitmap reads as unconfirmed, so every entry the capture recorded as
    /// agreed can be silently rewritten and
    /// [`Refused::Confirmed`](crate::Refused::Confirmed) never fires again --
    /// the log losing the authority the whole design rests on, and it loads
    /// clean. A partial row looks harmless for the opposite reason: nothing can
    /// reach it, because [`ActionLog::ticks`](crate::ActionLog::ticks) counts
    /// whole rows. It becomes reachable the moment the session records one more
    /// tick. [`Shape::Ragged`](crate::Shape::Ragged) is where that is spelled out.
    ///
    /// # Why this is public and not a constructor
    ///
    /// [`new`](Self::new) cannot produce an inconsistent session: it builds the
    /// log and the trace *from* the opening, and refuses the one opening it
    /// could not build them from. The disagreement arrives afterwards, through
    /// the public fields, which are public because a lockstep transport writes
    /// the log, a desync check reads the marks and a dev console reads the
    /// opening. Nothing can watch a `pub` field being assigned. So the check is
    /// a call anything that assembles a session by hand can make, rather than an
    /// invariant a type can hold, and [`seek`](Self::seek) says what it does
    /// with a session that never made it.
    ///
    /// # Errors
    ///
    /// One [`Shape`] per way, whichever is found first.
    pub fn check(&self) -> Result<(), Shape> {
        if self.log.first() != self.opening.first {
            return Err(Shape::LogStart {
                log: self.log.first(),
                opening: self.opening.first,
            });
        }
        if self.marks.first() != self.opening.first {
            return Err(Shape::TraceStart {
                trace: self.marks.first(),
                opening: self.opening.first,
            });
        }
        let Some(seats) = self.opening.seats() else {
            return Err(Shape::Roster {
                seats: self.opening.roster.len(),
            });
        };
        if self.log.players() != seats {
            return Err(Shape::Width {
                log: self.log.players(),
                roster: seats,
            });
        }
        let orphaned = match usize::from(self.log.players()) {
            0 => self.log.entries(),
            width => self.log.entries() % width,
        };
        if orphaned != 0 {
            return Err(Shape::Ragged {
                entries: self.log.entries(),
                players: self.log.players(),
            });
        }
        let needed = self.log.entries().div_ceil(8);
        if self.log.confirmed_bytes() != needed {
            return Err(Shape::Confirmations {
                bytes: self.log.confirmed_bytes(),
                needed,
                entries: self.log.entries(),
            });
        }
        Ok(())
    }
}
