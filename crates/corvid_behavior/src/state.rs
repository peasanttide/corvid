//! The deterministic half of a game: three data types and two functions, on the
//! state itself.

use core::fmt::Debug;
use core::hash::Hash;

use crate::{Command, Level, PlayerState};

/// What a value has to be to cross a wire, a disk or a digest.
///
/// The blanket implementation means a type never names this trait. It is a
/// bundle of the four things a simulation's data owes, and it is worth reading
/// as an obligation rather than as a bound.
///
/// **A round trip has to be faithful.** A `Serialize` that skips a field its
/// `Deserialize` expects, a `#[serde(skip)]` on something the state needs, a
/// `#[serde(into = "...")]` whose conversion loses precision -- these are the same
/// bug in different clothes. The tick that produced the state is deterministic
/// in every case, and the game still comes apart, because the state that
/// arrives is not the state that left.
/// [`round_trip_is_faithful`](../corvid_wire/fn.round_trip_is_faithful.html) is the mechanical
/// form of it, at one value: point it at the states a session actually reaches
/// and not at `State::default()`, which is the value a lost field decays to and
/// so the value most likely to survive anything.
///
/// **A clone has to be a copy.** `Clone` must give back a value that is `Eq` to
/// its source and digests the same. The derive does this; a hand-written
/// `Clone` that reseeds a field, resets a counter or drops a cache does not --
/// and a snapshot taken by cloning a state is only a snapshot if the clone is
/// one.
///
/// **No pointer-sized integer in hashed state.** A count that a peer compares
/// is a `u32` or a `u64`, never a `usize` or an `isize`. `corvid_hash`
/// overrides `write_usize` so a container's *length* prefix is eight bytes
/// everywhere, but `Hash::hash_slice` has a specialisation for integer slices
/// that hands over raw bytes and reaches past the override -- so a
/// `Vec<usize>` inside a state digests four bytes an element in a browser and
/// eight on a native server, and two peers that agree about everything else
/// disagree about the digest.
///
/// This bound cannot state that. Excluding a type from a blanket
/// implementation over `Hash` is not something a `where` clause can do, and
/// the alternative -- a `Digestible` trait of this workspace's own, deliberately
/// not implemented for `usize` -- costs every game a hand-written impl per type
/// to rule out a mistake a fixed-width annotation already rules out. So it is
/// an obligation rather than a bound, and it is written here because this is
/// where an implementor is reading.
///
/// # The encoding is the `serde` feature's
///
/// `Serialize + DeserializeOwned` are part of this bound when the feature is
/// on and absent when it is off. A game that never leaves one machine -- a
/// single-seat build with no saves, a test harness, a crate being compiled for
/// a target with no room for the format -- implements the trait without
/// writing an encoding, and the compiler stops asking for one it has no use
/// for.
///
/// What that costs is that the obligation above is not enforceable in such a
/// build, because there is nothing to round-trip. It is also not *reachable*
/// there: a state that is never written down cannot come back wrong. The
/// moment a build turns the feature on -- which every networked, saving or
/// replaying one does -- the bound is back and
/// [`round_trip_is_faithful`](../corvid_wire/fn.round_trip_is_faithful.html) is what
/// checks it.
#[cfg(feature = "serde")]
pub trait Data: serde::Serialize + serde::de::DeserializeOwned + Hash + Eq + Clone + Debug {}

#[cfg(feature = "serde")]
impl<T> Data for T where
    T: serde::Serialize + serde::de::DeserializeOwned + Hash + Eq + Clone + Debug
{
}

/// What a value has to be to cross a digest, with no encoding asked for.
///
/// The `serde` feature is off, so this is the same bundle as the documented
/// one minus `Serialize` and `DeserializeOwned`. See the other definition for
/// what an implementor owes; everything there still applies except the round
/// trip, which a build with no encoding cannot make.
#[cfg(not(feature = "serde"))]
pub trait Data: Hash + Eq + Clone + Debug {}

#[cfg(not(feature = "serde"))]
impl<T> Data for T where T: Hash + Eq + Clone + Debug {}

/// The deterministic half of a game.
///
/// # Implemented by the state, not by a marker
///
/// The arrangement the rest of the workspace is built around, and the one
/// argument for a marker type carrying five associated types is the orphan
/// rule: an art crate cannot implement a Corvid trait for a simulation crate's
/// type.
///
/// It does not apply, because a renderer implements nothing *for* the state. It
/// implements [`Extract<S>`](crate::Extract) for **its own** type, which its
/// own crate owns, and the state is a type parameter. So there is no marker,
/// and this is a trait a game puts on the struct it was already writing.
///
/// # What the signature buys, and what it does not
///
/// `tick` takes `self` by value and returns `Self`. There is no `&mut self` to
/// accumulate into and no sixth argument to hide state in, so everything the
/// tick is allowed to know is named in its signature.
///
/// That is a narrowing and not a proof. A method can still call
/// `Instant::now()`, read an environment variable, or load a `static
/// AtomicU64` that was set at startup, and no signature stops any of it.
/// Keeping a simulation crate `no_std` is a second narrowing and not a proof
/// either: it puts the clock, the environment and the filesystem out of easy
/// reach, and a `no_std` crate that writes `extern crate std` has them all
/// back.
///
/// A process-global with interior mutability survives both narrowings, and it
/// is the one leak no check inside a single process can find. What finds it is
/// two peers that are genuinely two processes comparing digests.
///
/// # What became of `Scratch`
///
/// It is gone. It was a memo channel into the tick, carrying an obligation -- "a
/// memo, never an accumulator" -- that no type could state and that a rollback
/// could silently violate: a scratch's value at tick N was a function of every
/// tick before it, and the runtime does not preserve that chain across a seek,
/// a rollback or a snapshot ring sized by one machine's spare memory.
///
/// What replaces it is `self` by value. A tick that wants to reuse an
/// allocation takes the `Vec` out of the state it was handed and puts it in the
/// state it returns, which is the same move without the channel that was
/// invisible to the hash.
pub trait State: Default + Data {
    /// What the game is called: the window's title, and the directory saves
    /// land in.
    ///
    /// Stated once rather than passed to a builder, because the two places it
    /// is read are a title bar and a path, and a game that spelled it
    /// differently in each would save into a directory nobody could find.
    const NAME: &'static str;

    /// Deterministic tuning. Every peer must agree; feeds the hash.
    ///
    /// This is the half of a game's settings that changes what the simulation
    /// computes. The other half -- resolution, volume, key bindings -- is the
    /// player's own machine, and it is a
    /// [`Controller::Config`](../corvid_control/trait.Controller.html#associatedtype.Config)
    /// rather than anything here.
    type Rules: Data;

    /// Authored, immutable within a session.
    type Level: Level;

    /// One player's intent for one tick. Goes on the wire. `Default` is idle.
    ///
    /// A player who did nothing submits the default, and a dropped player
    /// submits it forever, so a game never asks whether an action is present.
    type Action: Data + Default;

    /// Fold a newly loaded level into the state.
    ///
    /// Runs on **every peer at the same tick**, because the
    /// [`load`](Command::load) that asked for it came out of a tick every peer
    /// ran -- so this is part of the simulation, is hashed like any other part
    /// of it, and owes everything [`tick`](Self::tick) owes.
    ///
    /// `old` is [`None`] for the first level a session opens on.
    ///
    /// The default keeps the state as it was, which is right for a game whose
    /// state does not mention the level's shape. A game whose state holds
    /// positions on a map has to say what happens to them when the map changes,
    /// and this is where it says it.
    #[must_use]
    fn load_level(self, _old: Option<&Self::Level>, _new: &Self::Level) -> Self {
        self
    }

    /// The simulation. A pure function of the values its arguments denote: the
    /// same inputs produce the same state, bit for bit, on every machine,
    /// forever.
    ///
    /// # Values, not identities
    ///
    /// "Read only your arguments" is the rule a reader infers from the
    /// signature, and it is too weak. `Arc::strong_count(level)` reads nothing
    /// but an argument and is still peer-local: a peer whose runtime holds a
    /// second handle -- a deeper snapshot ring, a spectator feed, a recording --
    /// counts one more from the same level *value*. `Arc::as_ptr`,
    /// `players.as_ptr()`, and any ordering derived from an address are the
    /// same hole.
    ///
    /// So the obligation is the stricter one: **read only the values these
    /// arguments denote.** A level's contents, never its handle.
    ///
    /// # The sink
    ///
    /// `command` is where a tick reaches outside itself, and it reaches by
    /// describing rather than by doing. It is a `&mut impl` rather than a
    /// returned `Vec` for two reasons: a tick that asks for nothing -- which is
    /// almost all of them -- allocates nothing, and a test can pass a recorder
    /// and assert on what it was told.
    ///
    /// It also means this trait is not object-safe. Nothing uses it that way.
    #[must_use]
    fn tick(
        self,
        _level: &Self::Level,
        _players: &[PlayerState<Self::Action>],
        _rules: &Self::Rules,
        _command: &mut impl Command,
    ) -> Self {
        self
    }
}
