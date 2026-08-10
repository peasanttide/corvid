//! Who is playing, and the one thing each of them did this tick.

use corvid_time::Tick;

use corvid_macros::id_type;

id_type! {
    /// Which seat at the table, for the length of one session.
    ///
    /// Small and dense, because the runtime indexes an action log by it: one
    /// action per player per tick at `(tick - first) * players + player`. It is
    /// a seat rather than an account, so a player who drops and rejoins keeps
    /// theirs and the log stays rectangular.
    PlayerId, u16, "The seat number."
}

id_type! {
    /// Which account, for longer than one session.
    ///
    /// This is what a save file remembers and what a friends list matches on.
    /// It enters the simulation exactly once, in
    /// [`Presence::Joining`](crate::Presence::Joining), so a `State` can fold
    /// it in and thereafter talk about a [`PlayerId`].
    ProfileId, u64, "The account's identifier."
}

/// Where a player stands in the roster this tick.
///
/// The roster is the runtime's, not the game's. A game reads this to learn
/// that someone arrived or left; it never has to ask whether an action is
/// missing, because one is never missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub enum Presence {
    /// Joined on this tick, and on this tick only. A `State` folds `profile`
    /// in here, because this is the one tick it is offered.
    Joining {
        /// Whose account.
        profile: ProfileId,
    },
    /// Playing.
    Active,
    /// Still in the roster and submitting `Action::default()`, since the tick
    /// this happened on. May come back, which is why the seat is not reused.
    Dropped {
        /// The tick the player stopped submitting.
        since: Tick,
    },
}

/// One player, as the tick sees them.
///
/// Exactly one action per player per tick, always present. A player who did
/// nothing submits `Action::default()`, a dropped player submits it forever,
/// and "do we have every input for tick N" is a bit test rather than a search.
/// There is no `Option` here, no slice of actions, and no way to say "no
/// input", because every one of those would be a case a game could get wrong
/// and a case a rollback would have to reason about.
///
/// Attribution belongs to the runtime. Every action is attributed to the seat
/// it arrived on, and that seat is what [`id`](Self::id) holds, so a game never
/// has to ask who sent something -- the answer is already beside the action, put
/// there by the layer that authenticated it.
///
/// An `Action` is a game's own type, and nothing stops one from carrying a
/// [`PlayerId`] of its own. A game that reads attribution out of there is
/// trusting a number the sender chose, which is a cheating vector rather than a
/// desync: every peer folds the same lie in the same way, so the digests agree
/// and the seat that claimed to be another seat gets what it asked for. `id` is
/// the field the sender does not control.
///
/// # It owns its action
///
/// So this is [`Data`](crate::Data) like everything else a tick sees: it can be
/// written into a save, sent to a peer and digested on its own, rather than
/// being a view that has to be rebuilt from whatever the log still holds.
/// Holding `&'a A` instead would save a clone per player per tick and cost the
/// property that makes a roster recordable -- and the roster is one of the two
/// things a desync report has to be able to show, the other being the state.
///
/// The clone is the runtime's, once per player per tick, of a type a game
/// chose to be small enough to send over a network sixty times a second.
///
/// # There is no `pose` here, and the absence is the design
///
/// A `FineTransform` for head and hands would be the obvious fourth field, on
/// the grounds that XR poses are first class, and it cannot be one: an action
/// log records actions, so a seek has nothing to rebuild a pose from and would
/// hand every player the identity -- and a game whose tick read one would replay
/// to a different state than it ran, with nothing on the wire to blame and
/// nothing in the log to notice it with.
///
/// The rule that rules it out is worth naming, because it is the one that
/// decides what may ever be added here: **every input a tick can see has to be
/// in the log, or the session is not reproducible.** A seat number is derived
/// from the roster's order and a [`Presence`] is derived from the roster's join
/// and leave ticks, so those two survive a replay; a pose is derived from
/// nothing a capture holds.
///
/// So the fix is subtraction rather than a second recording. A game that wants
/// head and hand poses puts them in its own [`Action`](crate::State::Action),
/// where the log already carries them, the digest already covers them and a
/// rollback already corrects them. Nothing is lost -- a `FineTransform` costs
/// the same in an `Action` as it did here -- and an input that is not in the
/// input log stops being expressible.
///
/// The derived [`Hash`](core::hash::Hash) absorbs the seat, the presence and the
/// action, in that order. The runtime hashes the roster alongside the state so
/// that a desync caused by two peers disagreeing about *who did what* is
/// distinguishable from one caused by them disagreeing about what the simulation
/// made of it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct PlayerState<A> {
    /// Which seat.
    pub id: PlayerId,
    /// Where this player stands in the roster.
    pub presence: Presence,
    /// Exactly one, always present.
    pub action: A,
}

impl<A> PlayerState<A> {
    /// One seat's line in the roster for one tick.
    pub const fn new(id: PlayerId, presence: Presence, action: A) -> Self {
        Self {
            id,
            presence,
            action,
        }
    }
}
