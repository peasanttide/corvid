//! The vocabulary a tick uses to talk to the platform.

use crate::{PlayerId, id::id_type, name::bounded_name};

id_type! {
    /// What a process exits with.
    ExitCode, u8, "The status the operating system is handed."
}

impl ExitCode {
    /// The game ended the way it meant to. Zero, because that is what every
    /// shell, every CI runner and every process supervisor reads as success.
    pub const SUCCESS: Self = Self(0);
    /// It did not. One, the conventional unspecified failure; a game with
    /// something more specific to say constructs its own.
    pub const FAILURE: Self = Self(1);
}

id_type! {
    /// Which save to write or read.
    ///
    /// A number rather than a name because a slot is a position in a fixed set
    /// whose size the game decides, and a player picking "slot 3" in a menu is
    /// picking a position. What goes *in* the slot is the session and the state
    /// at the tick that asked, which the runtime already holds — so a game
    /// implements nothing to have saves, and the number is the whole of what it
    /// says.
    SaveSlot, u16, "Which slot."
}

id_type! {
    /// Which rumble effect, out of the set the game declared.
    RumbleId, u16, "The effect's index in that set."
}

id_type! {
    /// Which achievement, out of the set the game declared.
    ///
    /// Dense and numbered, not a store's string identifier. The platform layer
    /// owns the mapping from this to whatever Steam or a console calls it,
    /// because that name is a property of the shop a build was published to and
    /// not of the simulation that earned the achievement — and the simulation
    /// is the thing that has to digest identically on a peer published
    /// somewhere else.
    AchievementId, u16, "The achievement's index in that set."
}

id_type! {
    /// Which tracked statistic, out of the set the game declared. Numbered for
    /// the same reason [`AchievementId`] is.
    StatId, u16, "The statistic's index in that set."
}

id_type! {
    /// Which lobby, as the platform's networking layer names it.
    LobbyId, u64, "The identifier the platform handed out."
}

bounded_name! {
    /// The line a friends list shows under the player's name.
    ///
    /// Bounded at sixty-four bytes because every platform that displays one
    /// bounds it somewhere, and a limit the simulation enforces is a limit
    /// every peer agrees on.
    ///
    /// The bound buys the two things a `String` cannot: a fixed encoding, so
    /// the line digests and serializes identically on a peer whose platform
    /// would have truncated it somewhere else, and a refusal at the boundary,
    /// so a line that does not fit is an error where it was built rather than a
    /// quietly shorter line on one machine.
    ///
    /// It used to have to be boxed into a sixteen-byte `Command` variant as
    /// well. [`Command`] is a trait now, so an argument costs what it costs and
    /// nothing else pays for this one being wide.
    PresenceText, 64
}

bounded_name! {
    /// A link to open in whatever the platform considers a browser.
    ///
    /// Bounded at two hundred and fifty-six bytes, which is longer than any
    /// link a game has business opening and short enough to sit in a save file
    /// without anyone worrying about it. This crate neither parses nor
    /// validates the link; the platform layer that opens it is the layer that
    /// knows what it is willing to open. Like [`PresenceText`], the bound buys
    /// a fixed encoding and a refusal where the value was built.
    Url, 256
}

/// Who a request is addressed to: the session, or one machine.
///
/// # Why this survived the enum it used to describe
///
/// [`Command`] used to be a closed, `#[non_exhaustive]` enum, and this was
/// handed over by a `scope()` accessor. The accessor existed because a match on
/// a non-exhaustive enum is forced to write a fallback arm, and that arm was
/// holding an unknown request with no way to ask what kind it was.
///
/// One method per effect makes that problem disappear rather than solving it:
/// there is no fallback arm and no unknown request, because every effect is a
/// method whose scope is known where it is defined. So the accessor is gone and
/// this is not, because a runtime still records which kind of thing it routed —
/// and a record of a decision is worth more than a record of the input to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    /// About the session, so every peer emits it and every peer has to act on
    /// it. The runtime's job is to make sure they agree: a barrier where one is
    /// needed, and a single effect where the request would otherwise happen once
    /// per peer.
    Global,
    /// About one machine — its hardware, its window, its platform account — so
    /// the runtime routes it and everyone else ignores it. Every peer still
    /// *emits* it, because it came out of a deterministic tick; what differs is
    /// who acts.
    Local,
}

/// What a tick asks the platform for: one method per effect.
///
/// A request, not an action. It is called from inside the deterministic tick,
/// so every peer makes the same calls in the same order; the runtime agrees the
/// global ones across every peer and routes each local one to the machine it
/// belongs to. This is the only place the tick reaches outside itself, and it
/// reaches by describing rather than by doing.
///
/// # Why it is a trait and not a returned `Vec`
///
/// Two things, and the second is the one that mattered.
///
/// **A tick that asks for nothing allocates nothing.** That is almost all of
/// them. The old shape returned `Vec<Command<R>>` from every tick, and although
/// an empty `Vec` does not allocate, every element of every non-empty one was
/// as wide as the widest request — which is why the payloads that did not fit
/// beside a discriminant were boxed, and why this crate used to carry a doctest
/// asserting `size_of::<Command<()>>() <= 16`. None of that is needed now: an
/// argument list has no uniform width, so `set_presence` costs what a
/// `PresenceText` costs and every other method costs nothing.
///
/// **A test can be a `Vec`.** The sink is whatever the caller passes, so a test
/// passes a recorder and asserts on what it was told, and the runtime passes
/// the thing that actually writes the file. That is the whole reason the shape
/// changed.
///
/// # Every method has a default that does nothing
///
/// So a sink implements only what it has code for. The runtime's implementation
/// overrides the methods it can act on and leaves the rest, and a default that
/// did nothing is exactly what
/// [`Answer::Unhandled`](../corvid_app/enum.Answer.html#variant.Unhandled)
/// records: a game that asks for a lobby on a runtime with no lobbies still
/// gets a record and a warning rather than a silent drop.
///
/// # Rumble is not here
///
/// It is the controller's, extracted from the state the way sound is. A haptic
/// routed through a deterministic tick would go on the wire and wait for a
/// network round trip in order to reach a device exactly one machine has —
/// which is the same argument the camera and the pointer are client-local for,
/// and it has the same answer.
pub trait Command {
    /// How the game names a level, which the two requests that name one carry.
    ///
    /// This is `<S::Level as Level>::Reference` at every real call site. It is
    /// an associated type rather than a parameter so that a sink is written
    /// against one game's levels and cannot be handed another's.
    type Reference;

    /// Load a level. **Global.**
    ///
    /// The simulation does not advance past this tick until the level is in
    /// hand. That rule is a function of the state, so every peer applies it and
    /// each sits there for a different number of milliseconds — and a peer that
    /// has stopped ticking submits no actions, so every other peer stalls
    /// inside its prediction window. The cross-peer barrier is the input
    /// dependency that was already there; nothing new goes on the wire.
    fn load(&mut self, _reference: Self::Reference) {}

    /// Drop a level the simulation is finished with. **Global.**
    fn unload(&mut self, _reference: Self::Reference) {}

    /// Stop, with this status. **Global.**
    fn quit(&mut self, _code: ExitCode) {}

    /// How fast game time passes. **Global.**
    ///
    /// [`ONE`](corvid_fixed::Factor16::ONE) is real time,
    /// [`ZERO`](corvid_fixed::Factor16::ZERO) is stopped, two is double speed.
    ///
    /// It scales the wall-clock-to-tick conversion, so **each tick's own step
    /// is unchanged and no digest moves**: the same session played at one and
    /// at two produces the same states, tick for tick, and only the wall clock
    /// differs. What changes is how many ticks a real second buys.
    ///
    /// Not to be confused with the interpolation weight a renderer lerps with,
    /// which is `draw`'s own `alpha`. The two are one word apart and mean
    /// entirely different things.
    fn set_time_factor(&mut self, _factor: corvid_fixed::Factor16) {}

    /// Write a save. **Global.**
    ///
    /// # No bytes
    ///
    /// What a save writes is the session and the state, both of which the
    /// runtime holds. The blob a tick used to hand over here had no route back
    /// into a simulation on reload — nothing read it, and nothing could have —
    /// so it was a `Vec<u8>` allocated on every save and dropped on every load.
    fn save(&mut self, _slot: SaveSlot) {}

    /// Ask whether there is a save in a slot, which is what a menu of them
    /// needs to know. **Global.**
    ///
    /// Nothing arrives back inside the tick, because a tick cannot wait for
    /// anything; what the runtime answers is recorded beside the request.
    fn read(&mut self, _slot: SaveSlot) {}

    /// Capture the frame. **Local**: only one machine is drawing it.
    fn screenshot(&mut self) {}

    /// Ask the platform's overlay to invite someone. **Local**, to the peer
    /// that owns the player it names.
    fn invite(&mut self, _player: PlayerId) {}

    /// Join a lobby. **Global**: a lobby is the session's identity on the
    /// platform's network, and half a session in it is a split session.
    fn join_lobby(&mut self, _lobby: LobbyId) {}

    /// Leave whichever lobby this peer is in. **Global**, for the same reason.
    fn leave_lobby(&mut self) {}

    /// Set the line a friends list shows. **Local**: it writes to one
    /// platform account.
    fn set_presence(&mut self, _presence: PresenceText) {}

    /// Open a link outside the game. **Local**, and the failure mode is worth
    /// naming: every peer acting on this is a game that opened a browser on
    /// four machines because one player pressed a button.
    fn open_url(&mut self, _url: Url) {}

    /// Award an achievement. **Local**: a platform account.
    fn achieve(&mut self, _achievement: AchievementId) {}

    /// Set a tracked statistic. **Local**, for the same reason.
    fn stat(&mut self, _id: StatId, _value: i64) {}
}

/// A sink that drops everything, for a tick whose requests nobody is listening
/// for.
///
/// For a test asserting on a state rather than on what was asked for, and for a
/// caller replaying ticks whose effects have already happened.
///
/// It is generic over the reference rather than being `impl Command for ()`,
/// and that is not a stylistic choice: `()` could only ever have
/// `Reference = ()`, so it would be a sink no game could use unless its levels
/// were also `()`. The parameter is inferred at the call site from the state
/// being ticked.
///
/// ```
/// use corvid_behavior::Discard;
///
/// let mut nobody = Discard::<String>::new();
/// # let _ = &mut nobody;
/// ```
#[derive(Debug)]
pub struct Discard<R>(core::marker::PhantomData<fn() -> R>);

impl<R> Discard<R> {
    /// A sink that listens to nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self(core::marker::PhantomData)
    }
}

impl<R> Default for Discard<R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R> Command for Discard<R> {
    type Reference = R;
}
