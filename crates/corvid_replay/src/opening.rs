//! What a session starts from: the seed, the roster, and the opening itself.
//!
//! Split from [`session`](crate::session) because the two are read at different
//! moments. An opening is what a game writes down before anything has happened;
//! a session is what that becomes once it has. Everything here is fixed for the
//! life of a session, which is what makes it the half a capture can be checked
//! against before a single tick is replayed.

use alloc::{string::String, sync::Arc, vec::Vec};
use core::hash::Hash;

use corvid_behavior::{PlayerId, Presence, ProfileId, State};
use corvid_hash::Digest;
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

/// The number a game seeds its own randomness from.
///
/// It is recorded in the opening, and [`tick`](State::tick) never receives it
/// -- the signature has no argument it could arrive through. Whoever builds the
/// opening folds it into [`Opening::origin`], which is where a replay reads it
/// back out of, so this field is the record of what the session was opened with
/// rather than the route it takes into the simulation.
///
/// Nothing hashes it. An [`Opening`] has no [`Hash`] impl and no digest of its
/// own; the one digest it carries is [`schema`](Opening::schema), which is
/// about the *types* rather than about the values. So a peer that opened with a
/// different seed is told apart by the first mark, because the origin state it
/// seeded is one of the two things that mark is taken of -- the level being the
/// other -- and not by a comparison of openings, which nothing here performs.
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
/// did -- it is not in the log, and a state that folds a profile in on the
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
/// but [`Session::seek`](crate::Session::seek) takes no level argument, so a session that carried
/// only the name could not run a tick without one being handed to it from
/// somewhere else. Carrying the content is what makes a seek a function of the
/// session alone. It costs a copy of the level in every capture, which is the
/// trade and is stated here rather than discovered.
///
/// [`origin`](Self::origin) is the state at [`first`](Self::first). Without it
/// "replay from the opening" has nothing to replay from: `State::State` is
/// not `Default` and a seek to `first` would have no value to return. It is
/// also what makes seeking independent of the snapshot ring -- the ring can be
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
/// value. [`Session::forget_before`](crate::Session::forget_before) swaps a new origin in, and what the runtime
/// has to swap in is the state it is currently displaying -- which it is holding
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
    pub level: String,
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
    /// [`forget_before`](crate::Session::forget_before) replaces.
    ///
    /// [`None`] means [`S::default()`](Default::default), which is what a fresh
    /// session opens on. `State` is bounded by `Default` precisely so that this
    /// can be optional: a game folds whatever its opening position is into its
    /// own `Default`, and nothing has to be supplied to start playing.
    ///
    /// The override is for the three cases that genuinely have a state to open
    /// from -- a save, a replay, and a peer joining a session already in
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
    /// the same file should disagree at the first mark -- with the reference in
    /// the report -- rather than once the contents start mattering.
    ///
    /// It is a method rather than two lines at each site because there are two
    /// sites: the trace a live session opens with, and the trace a replay
    /// recomputes to compare against it. The two disagreeing would report every
    /// capture in the workspace as diverged at tick zero.
    ///
    /// The **resolved** origin, not the [`Option`] field. An `Option`'s [`Hash`]
    /// writes a discriminant before its payload, so digesting the field directly
    /// would make this depend on whether a session stated its origin or let it
    /// default -- two ways of saying the same state, hashing differently.
    #[must_use]
    pub fn mark(&self) -> Digest {
        let mut hasher = corvid_hash::Hasher::new();
        self.origin().hash(&mut hasher);
        self.content.hash(&mut hasher);
        hasher.digest()
    }

    /// How many seats the roster has, or [`None`] for a roster no [`PlayerId`](corvid_behavior::PlayerId)
    /// can name.
    ///
    /// A seat number is a `u16` and the log indexes by it, so a roster longer
    /// than [`u16::MAX`] has seats no action can be attributed to. Saturating
    /// here would be worse than answering nothing: [`Session::new`](crate::Session::new) would build
    /// a log 65 535 wide for a roster naming seventy thousand, which is the
    /// exact disagreement [`Session::check`](crate::Session::check) exists to refuse, and every caller
    /// would have to distrust a number that looks like a width. So the
    /// impossible case has a value of its own, and the two callers that need a
    /// width to proceed report [`Shape::Roster`](crate::Shape::Roster) instead of inventing one.
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
