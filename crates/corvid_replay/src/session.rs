//! Everything needed to reproduce a session, and the two functions that write
//! it down and read it back.

use alloc::{sync::Arc, vec::Vec};
use core::{fmt, hash::Hash};

use corvid_behavior::{Level, PlayerId, Presence, ProfileId, State};
use corvid_hash::Digest;
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

use crate::{ActionLog, HashTrace};

/// How a state's level names itself, spelled once.
///
/// `<S::Level as Level>::Reference` appears in an opening, in a save and in
/// every `Command::load`, and writing it out at each site buries what the field
/// actually is.
pub type LevelRef<S> = <<S as State>::Level as Level>::Reference;

/// The number a game seeds its own randomness from.
///
/// It is recorded in the opening, and [`tick`](State::tick) never receives it
/// — the signature has no argument it could arrive through. Whoever builds the
/// opening folds it into [`Opening::origin`], which is where a replay reads it
/// back out of, so this field is the record of what the session was opened with
/// rather than the route it takes into the simulation.
///
/// Nothing hashes it. An [`Opening`] has no [`Hash`] impl and no digest of its
/// own; the one digest it carries is [`schema`](Opening::schema), which is
/// about the *types* rather than about the values. So a peer that opened with a
/// different seed is told apart by the first mark, because the origin state it
/// seeded is one of the two things that mark is taken of — the level being the
/// other — and not by a comparison of openings, which nothing here performs.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
// A statement of intent rather than a load-bearing attribute: `serde` writes a
// newtype struct as its inner value with or without it, so nothing observable
// changes if it goes. `tests/names.rs` says which of the two claims about this
// the golden row actually supports.
#[serde(transparent)]
pub struct Seed(
    /// The bits.
    pub u64,
);

/// One seat in a session's roster, and when it was occupied.
///
/// The seat number is the profile's position in [`Opening::roster`] and is not
/// stored: the action log indexes by that position, so a `seat` field would be
/// a second copy of an index that has to agree with the first. A roster of `n`
/// profiles is seats `PlayerId(0)` through `PlayerId(n - 1)`, in order.
///
/// [`joined`](Self::joined) and [`left`](Self::left) are what a replay
/// reconstructs [`Presence`] from, so the roster carries the whole of the
/// presence timeline and the log carries none of it. That is the only reason a
/// replay can produce `Presence::Joining` on exactly the tick the live session
/// did — it is not in the log, and a state that folds a profile in on the
/// joining tick would otherwise never fold it in twice the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Profile {
    /// Whose account.
    pub account: ProfileId,
    /// The tick this seat joined on, which is the one tick it is
    /// [`Presence::Joining`] for.
    pub joined: Tick,
    /// The tick this seat stopped submitting, if it has. From that tick on it
    /// is [`Presence::Dropped`] and submits the default action forever.
    pub left: Option<Tick>,
}

impl Profile {
    /// Where this seat stands at `tick`, or [`None`] before it joined.
    ///
    /// A seat that has not joined yet is not in the roster the tick sees at
    /// all. `Presence` has three cases and none of them is "not here", which is
    /// deliberate: a game reads a roster of players who exist, and a seat that
    /// has not arrived is simply absent from the slice rather than present with
    /// a fourth state to handle.
    #[must_use]
    pub fn presence_at(&self, tick: Tick) -> Option<Presence> {
        if tick < self.joined {
            return None;
        }
        if let Some(since) = self.left
            && tick >= since
        {
            return Some(Presence::Dropped { since });
        }
        if tick == self.joined {
            return Some(Presence::Joining {
                profile: self.account,
            });
        }
        Some(Presence::Active)
    }
}

/// Everything a session starts from, and everything it needs to start again.
///
/// # Two fields a level name alone does not give, and why they are here
///
/// [`content`](Self::content) is the level itself and not only its
/// [`name`](Self::level). `corvid_behavior` describes a level as hashed into
/// the opening and sent as a name, and that is what [`level`](Self::level) is;
/// but [`Session::seek`] takes no level argument, so a session that carried
/// only the name could not run a tick without one being handed to it from
/// somewhere else. Carrying the content is what makes a seek a function of the
/// session alone. It costs a copy of the level in every capture, which is the
/// trade and is stated here rather than discovered.
///
/// [`origin`](Self::origin) is the state at [`first`](Self::first). Without it
/// "replay from the opening" has nothing to replay from: `State::State` is
/// not `Default` and a seek to `first` would have no value to return. It is
/// also what makes seeking independent of the snapshot ring — the ring can be
/// empty and the opening is still a place to start.
///
/// # Why those two and the level are handles
///
/// [`content`](Self::content), [`rules`](Self::rules) and
/// [`origin`](Self::origin) are all [`Arc`], and they agree for one reason:
/// each of them is a value the client-local half of the game is handed every
/// displayed frame. A runtime that held any of these three by value would
/// deep-clone a whole level or a whole state to hand over something nobody
/// mutates, several times a second, forever. Behind a
/// handle it is a refcount bump.
///
/// [`origin`](Self::origin) has a second reason it could not have stayed a
/// value. [`Session::forget_before`] swaps a new origin in, and what the runtime
/// has to swap in is the state it is currently displaying — which it is holding
/// as a handle, because that is what it hands the frame. A by-value parameter
/// there would force it to clone the state it already has, in the one call whose
/// whole purpose is to stop holding memory.
///
/// None of this is visible in a capture. `Arc`'s serde and [`Hash`]
/// implementations read through to what they point at, and the impls below go
/// further and write the values by hand, so an opening's bytes and every digest
/// taken of one are what they were when these were three plain fields.
pub struct Opening<S: State> {
    /// Which authored level, as the game names one. This is what a capture is
    /// identified by and what a [`Command::load`](corvid_behavior::Command::load)
    /// would name.
    pub level: LevelRef<S>,
    /// The level itself, so that a seek needs nothing but the session. This is
    /// the handle [`tick`](State::tick) is passed and the one a frame
    /// carries.
    pub content: Arc<S::Level>,
    /// The tuning every peer has to agree on.
    pub rules: Arc<S::Rules>,
    /// Who is playing, seat by seat. The position in this vector is the seat
    /// number, and the length is how wide the action log's rows are.
    pub roster: Vec<Profile>,
    /// What the game seeded its randomness from, folded into
    /// [`origin`](Self::origin) by whoever built this.
    pub seed: Seed,
    /// The first tick of the session.
    pub first: Tick,
    /// The state at [`first`](Self::first), and what a
    /// [`forget_before`](Session::forget_before) replaces.
    ///
    /// [`None`] means [`S::default()`](Default::default), which is what a fresh
    /// session opens on. `State` is bounded by `Default` precisely so that this
    /// can be optional: a game folds whatever its opening position is into its
    /// own `Default`, and nothing has to be supplied to start playing.
    ///
    /// The override is for the three cases that genuinely have a state to open
    /// from — a save, a replay, and a peer joining a session already in
    /// progress. [`origin`](Self::origin) resolves the two into one handle.
    pub origin: Option<Arc<S>>,
    /// A digest of the game's type schema, compared by
    /// [`Session::load`](crate::Session::load) so that a capture from an
    /// incompatible build refuses to load rather than diverging silently. See
    /// [`Schema`](crate::Schema) for what this can and cannot tell apart.
    pub schema: Digest,
}

impl<S: State> Opening<S> {
    /// The state this session opens on: whatever was supplied, or
    /// [`S::default()`](Default::default).
    #[must_use]
    pub fn origin(&self) -> Arc<S> {
        self.origin.clone().unwrap_or_default()
    }

    /// The mark a session's trace opens on.
    ///
    /// **Not a state's digest**, which every other mark in a trace is. This one
    /// covers the origin *and* the level, because both are starting conditions
    /// two peers have to agree about, and a peer holding a different build of
    /// the same file should disagree at the first mark — with the reference in
    /// the report — rather than once the contents start mattering.
    ///
    /// It is a method rather than two lines at each site because there are two
    /// sites: the trace a live session opens with, and the trace a replay
    /// recomputes to compare against it. The two disagreeing would report every
    /// capture in the workspace as diverged at tick zero.
    ///
    /// The **resolved** origin, not the [`Option`] field. An `Option`'s [`Hash`]
    /// writes a discriminant before its payload, so digesting the field directly
    /// would make this depend on whether a session stated its origin or let it
    /// default — two ways of saying the same state, hashing differently.
    #[must_use]
    pub fn mark(&self) -> Digest {
        let mut hasher = corvid_hash::Hasher::new();
        self.origin().hash(&mut hasher);
        self.content.hash(&mut hasher);
        hasher.digest()
    }

    /// How many seats the roster has, or [`None`] for a roster no [`PlayerId`]
    /// can name.
    ///
    /// A seat number is a `u16` and the log indexes by it, so a roster longer
    /// than [`u16::MAX`] has seats no action can be attributed to. Saturating
    /// here would be worse than answering nothing: [`Session::new`] would build
    /// a log 65 535 wide for a roster naming seventy thousand, which is the
    /// exact disagreement [`Session::check`] exists to refuse, and every caller
    /// would have to distrust a number that looks like a width. So the
    /// impossible case has a value of its own, and the two callers that need a
    /// width to proceed report [`Shape::Roster`] instead of inventing one.
    #[must_use]
    pub fn seats(&self) -> Option<u16> {
        u16::try_from(self.roster.len()).ok()
    }

    /// The seat at `index`, if the roster has one.
    #[must_use]
    pub fn seat(&self, player: PlayerId) -> Option<&Profile> {
        self.roster.get(usize::from(player.0))
    }
}

/// Everything needed to reproduce a session bit for bit.
///
/// The log is the game. The state at any tick is a function of the opening and
/// the actions up to it, which is what makes save, load, replay, rollback and
/// time-walk one operation — see [`seek`](Self::seek).
///
/// The three fields are public because every one of them is read and written by
/// a layer this crate does not contain: a lockstep transport writes actions
/// into the log, a desync check reads the marks, a dev console reads the
/// opening. What that costs is that the three can be put out of step by hand —
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
/// error type that says what it refused. [`Session::new`] is the same call
/// under the name a constructor is looked for by.
impl<S: State> TryFrom<Opening<S>> for Session<S> {
    type Error = Shape;

    /// # Errors
    ///
    /// [`Shape::Roster`], for an opening whose roster is wider than a
    /// [`PlayerId`] can address, which is the only way this can fail.
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
    /// [`Shape::Roster`], for an opening whose roster is wider than a
    /// [`PlayerId`] can address. That is the only way this can fail, and it
    /// fails rather than narrowing: a log is as wide as [`Opening::seats`], so
    /// a roster of seventy thousand would get 65 535 columns and leave 4 465
    /// seats with nowhere to put an action. A session with more seats than its
    /// log can address is not a session — [`check`](Self::check) refuses
    /// exactly that capture — and a constructor that returned one would be
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
    /// **That `origin` is the state at `tick`.** Nothing here can recompute it —
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
    /// [`Arc`] — it is the one it hands to every frame — so the argument is a
    /// refcount bump rather than a copy of a whole simulation state. The old
    /// origin comes back as the handle the session was holding, which is the
    /// only sense in which it is "the" state at the old first tick: a snapshot
    /// ring, a frame still on screen or a peer's rollback buffer may be holding
    /// the same value, and this call says nothing about whether the caller has
    /// the last handle to it.
    ///
    /// It comes back rather than being dropped here because letting go of it is
    /// the caller's to decide. A runtime that still wants the old origin — to
    /// compare against, to write out, to keep one more displayed frame — would
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
    /// [`round_trip_is_faithful`](corvid_behavior::round_trip_is_faithful) is
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
    /// or the trace can, the roster can name more seats than a [`PlayerId`] can
    /// address, the log's rows can be a different width than the roster, the
    /// log's entries can stop partway through a row, and the log's confirmation
    /// bitmap can be a different length than its actions need.
    ///
    /// The last two are the ones that do not announce themselves. A short
    /// bitmap reads as unconfirmed, so every entry the capture recorded as
    /// agreed can be silently rewritten and
    /// [`Refused::Confirmed`](crate::Refused::Confirmed) never fires again —
    /// the log losing the authority the whole design rests on, and it loads
    /// clean. A partial row looks harmless for the opposite reason: nothing can
    /// reach it, because [`ActionLog::ticks`](crate::ActionLog::ticks) counts
    /// whole rows. It becomes reachable the moment the session records one more
    /// tick. [`Shape::Ragged`] is where that is spelled out.
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

/// A capture could not be turned into a session this build can replay.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Load {
    /// The bytes are not a session, with the encoder's reason.
    Bytes(corvid_wire::Error),
    /// The capture was recorded by a build that describes its types
    /// differently.
    ///
    /// This is the refusal the schema exists for: replaying a session under a
    /// build whose `State` means something else produces a state that is wrong
    /// without being detectably wrong, and the first thing that notices is a
    /// peer, later, disagreeing about a digest.
    Schema {
        /// What the capture says the build that wrote it was.
        recorded: Digest,
        /// What this build says it is.
        running: Digest,
    },
    /// The capture's own parts disagree about the session they describe.
    Shape(Shape),
}

impl fmt::Display for Load {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bytes(why) => write!(f, "the bytes are not a session: {why}"),
            Self::Schema { recorded, running } => write!(
                f,
                "this capture was recorded by a build describing itself as \
                 {recorded} and this build describes itself as {running}: \
                 replaying it would not reproduce the session it recorded"
            ),
            Self::Shape(shape) => write!(f, "the capture's parts disagree: {shape}"),
        }
    }
}

impl core::error::Error for Load {}

/// A session would not forget to a tick.
///
/// Both cases are a tick outside the stretch the session covers, and neither is
/// about how much of it is worth keeping: forgetting to a tick a session already
/// opens at is legal and does nothing, which is what lets a runtime call it on a
/// schedule without first asking where it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Forget {
    /// Before the opening. There is nothing there to forget.
    Early {
        /// The tick that was asked for.
        tick: Tick,
        /// The tick the session opens on.
        first: Tick,
    },
    /// Past the last tick the log reaches, so the session has no state there to
    /// be told about — and forgetting to it would drop rows whose states nobody
    /// has computed yet.
    Beyond {
        /// The tick that was asked for.
        tick: Tick,
        /// The latest tick the session's log reaches.
        last: Tick,
    },
}

impl fmt::Display for Forget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Early { tick, first } => write!(
                f,
                "tick {tick} is before the session's opening tick {first}, so \
                 there is nothing before it to forget"
            ),
            Self::Beyond { tick, last } => write!(
                f,
                "tick {tick} is past tick {last}, which is as far as this \
                 session's log reaches: forgetting to it would drop rows whose \
                 states nothing has computed"
            ),
        }
    }
}

impl core::error::Error for Forget {}

/// Which of a capture's parts disagreed with which.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Shape {
    /// The log starts at a different tick than the opening.
    LogStart {
        /// The log's first tick.
        log: Tick,
        /// The opening's.
        opening: Tick,
    },
    /// The trace starts at a different tick than the opening.
    TraceStart {
        /// The trace's first tick.
        trace: Tick,
        /// The opening's.
        opening: Tick,
    },
    /// The roster names more seats than a [`PlayerId`] can address.
    ///
    /// A seat number is a `u16`, so a roster of more than sixty-five thousand
    /// has seats no action can be attributed to. It is the one shape check that
    /// is about a type's range rather than about two parts disagreeing.
    Roster {
        /// How many seats the roster names.
        seats: usize,
    },
    /// The log's rows are not as wide as the roster.
    Width {
        /// How many seats a row of the log holds.
        log: u16,
        /// How many the roster names.
        roster: u16,
    },
    /// The log's entries stop partway through a row.
    ///
    /// This is the one that looks like a few wasted bytes and is not.
    /// [`ActionLog::ticks`](crate::ActionLog::ticks) counts whole rows, so the
    /// entries past the last one are unreachable through every accessor *while
    /// the log stays this length* — and they are not off to one side, they are
    /// the front of the next row. The first
    /// [`extend_to`](crate::ActionLog::extend_to) makes that row exist, and it
    /// arrives already holding those entries, with whatever confirmation bits
    /// the capture set for them. From that tick on the session simulates
    /// actions nobody recorded for seats nobody played, and the peers sending
    /// the real ones are turned away with
    /// [`Refused::Confirmed`](crate::Refused::Confirmed).
    Ragged {
        /// How many entries the log holds.
        entries: usize,
        /// How many seats wide a row is.
        players: u16,
    },
    /// The log's confirmation bitmap is not as long as its entries need.
    ///
    /// This is the one that costs the log its authority rather than its
    /// indexing. A bit past the end of the bitmap reads as zero, so every entry
    /// it does not cover is *unconfirmed* — and an unconfirmed entry can be
    /// written to. A capture that arrived a byte short would let a peer rewrite
    /// actions the session has already agreed on and simulated, one at a time,
    /// with no refusal anywhere.
    Confirmations {
        /// How many bytes the bitmap holds.
        bytes: usize,
        /// How many the entries need.
        needed: usize,
        /// How many entries the log holds.
        entries: usize,
    },
}

impl fmt::Display for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LogStart { log, opening } => write!(
                f,
                "the log's first row is tick {log} and the session opens at \
                 {opening}, so every row would be read against the wrong tick"
            ),
            Self::TraceStart { trace, opening } => write!(
                f,
                "the trace's first mark is tick {trace} and the session opens at \
                 {opening}, so every mark would be compared against the wrong tick"
            ),
            Self::Roster { seats } => write!(
                f,
                "the roster names {seats} seats and a seat number is a u16, so \
                 everything past {} has no action that could be attributed to it",
                u16::MAX,
            ),
            Self::Width { log, roster } => write!(
                f,
                "a row of the log holds {log} seats and the roster names \
                 {roster}, so every row after the first would be read against \
                 the wrong seats"
            ),
            Self::Ragged { entries, players } => write!(
                f,
                "the log holds {entries} entries in rows of {players}, which is \
                 not a whole number of rows: the entries past the last whole \
                 row are the front of the next one the log grows, where they \
                 would be read as actions this capture never recorded"
            ),
            Self::Confirmations {
                bytes,
                needed,
                entries,
            } => write!(
                f,
                "the log's confirmation bitmap holds {bytes} bytes and its \
                 {entries} entries need {needed}, so the entries it does not \
                 cover read as unconfirmed and anything could be written over \
                 what the session already agreed on"
            ),
        }
    }
}

impl core::error::Error for Shape {}

// Every derive below would put a bound on `G` — `G: Clone`, `G: Serialize` —
// and `G` is a marker type with no fields that satisfies none of them. What
// these types are made of is `G`'s associated types, all four of which are
// `Data`, so the bounds the derives want are already in the `State` bound
// and the ones they would add are wrong. Hence `#[serde(bound = "")]` and the
// hand-written rest.

impl<S: State> Serialize for Opening<S> {
    fn serialize<Ser: serde::Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        #[derive(Serialize)]
        #[serde(bound = "")]
        struct Wire<'a, S: State> {
            level: &'a LevelRef<S>,
            /// The value and not the handle, for this field and for `rules` and
            /// `origin` below.
            ///
            /// The three of them are [`Arc`]s in the struct, and this shim is
            /// what pins the encoding to what those three values are rather than
            /// to how they happen to be held. `serde`'s `rc` feature is on in
            /// this build — `Session::seek` hands back an `Arc<S>` and
            /// something has to be able to write one down — so the naive
            /// derive would now compile and would produce the same bytes today,
            /// since `serde`'s `Arc` implementations read straight through to
            /// what they point at. What it would give up is the guarantee: the
            /// format of a capture would then be a property of a feature flag
            /// and of an upstream crate's choices about handles, and this is the
            /// one type in the workspace where that has to be a property of the
            /// source instead. The `&'a` fields deref-coerce out of the `Arc`s
            /// with no cast anywhere, so keeping them costs nothing and saying
            /// so here is the whole of the maintenance burden.
            content: &'a S::Level,
            rules: &'a S::Rules,
            roster: &'a [Profile],
            seed: &'a Seed,
            first: &'a Tick,
            origin: &'a S,
            schema: u64,
        }

        // Resolved rather than optional on the wire: a written-down session
        // always opens on a definite state, so the file format is unchanged by
        // the field having become an `Option` in memory.
        let origin = self.origin();

        Wire::<S> {
            level: &self.level,
            content: &self.content,
            rules: &self.rules,
            roster: &self.roster,
            seed: &self.seed,
            first: &self.first,
            origin: &origin,
            schema: self.schema.to_u64(),
        }
        .serialize(serializer)
    }
}

impl<'de, S: State> Deserialize<'de> for Opening<S> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(bound = "")]
        struct Wire<S: State> {
            level: LevelRef<S>,
            content: S::Level,
            rules: S::Rules,
            roster: Vec<Profile>,
            seed: Seed,
            first: Tick,
            origin: S,
            schema: u64,
        }

        let wire = Wire::<S>::deserialize(deserializer)?;
        Ok(Self {
            level: wire.level,
            content: Arc::new(wire.content),
            rules: Arc::new(wire.rules),
            roster: wire.roster,
            seed: wire.seed,
            first: wire.first,
            origin: Some(Arc::new(wire.origin)),
            schema: Digest::from_u64(wire.schema),
        })
    }
}

impl<S: State> Clone for Opening<S> {
    fn clone(&self) -> Self {
        Self {
            level: self.level.clone(),
            content: Arc::clone(&self.content),
            rules: Arc::clone(&self.rules),
            roster: self.roster.clone(),
            seed: self.seed,
            first: self.first,
            origin: self.origin.clone(),
            schema: self.schema,
        }
    }
}

impl<S: State> fmt::Debug for Opening<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Opening")
            .field("level", &self.level)
            .field("content", &self.content)
            .field("rules", &self.rules)
            .field("roster", &self.roster)
            .field("seed", &self.seed)
            .field("first", &self.first)
            .field("origin", &self.origin)
            .field("schema", &self.schema)
            .finish()
    }
}

/// Equal values, never equal handles.
///
/// The three [`Arc`] fields are dereferenced rather than compared as handles, so
/// an opening that has been through a capture — which rebuilds every one of them
/// as a fresh allocation — is equal to the one that was written down. That is
/// what `tests/roundtrip.rs` asserts, and it would be an assertion about
/// addresses if this compared what it holds rather than what it points at.
impl<S: State> PartialEq for Opening<S> {
    fn eq(&self, other: &Self) -> bool {
        self.level == other.level
            && *self.content == *other.content
            && *self.rules == *other.rules
            && self.roster == other.roster
            && self.seed == other.seed
            && self.first == other.first
            // Resolved on both sides, so an opening that carried no origin and
            // one that carried `S::default()` explicitly compare equal — which
            // is right, because they open the same session.
            && *self.origin() == *other.origin()
            && self.schema == other.schema
    }
}

impl<S: State> Eq for Opening<S> {}

impl<S: State> Serialize for Session<S> {
    fn serialize<Ser: serde::Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        #[derive(Serialize)]
        #[serde(bound = "")]
        struct Wire<'a, S: State> {
            opening: &'a Opening<S>,
            log: &'a ActionLog<S::Action>,
            marks: &'a HashTrace,
        }

        Wire::<S> {
            opening: &self.opening,
            log: &self.log,
            marks: &self.marks,
        }
        .serialize(serializer)
    }
}

impl<'de, S: State> Deserialize<'de> for Session<S> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(bound = "")]
        struct Wire<S: State> {
            opening: Opening<S>,
            log: ActionLog<S::Action>,
            marks: HashTrace,
        }

        let wire = Wire::<S>::deserialize(deserializer)?;
        Ok(Self {
            opening: wire.opening,
            log: wire.log,
            marks: wire.marks,
        })
    }
}

impl<S: State> Clone for Session<S> {
    fn clone(&self) -> Self {
        Self {
            opening: self.opening.clone(),
            log: self.log.clone(),
            marks: self.marks.clone(),
        }
    }
}

impl<S: State> fmt::Debug for Session<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("opening", &self.opening)
            .field("log", &self.log)
            .field("marks", &self.marks)
            .finish()
    }
}

impl<S: State> PartialEq for Session<S> {
    fn eq(&self, other: &Self) -> bool {
        self.opening == other.opening && self.log == other.log && self.marks == other.marks
    }
}

impl<S: State> Eq for Session<S> {}
