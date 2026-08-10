//! What a level is, and how one is read.

use crate::Data;

/// Authored, immutable within a session, hashed into the opening.
///
/// Behind an [`Arc`](alloc::sync::Arc) at the call site, so switching levels is
/// a pointer swap and a snapshot ring does not hold a copy per tick.
pub trait Level: Data {
    /// Why a level could not be read.
    ///
    /// The game's own type, because this crate does not know what reading one
    /// involves: a game with a fixed set of levels fails only by being handed a
    /// name it does not have, one that reads files fails the way a filesystem
    /// does, and one that fetches over a network fails in a third way. What the
    /// runtime needs of it is that it can be reported, which is what the bound
    /// says and the whole of what the bound says.
    type Error: core::error::Error;

    /// Read the level this name refers to.
    ///
    /// # A name, not a path
    ///
    /// The name is whatever the game calls a level: `"terminus"`, `"2-4"`, a
    /// relative path if that is what a game wants. It is a `&str` because every
    /// place one is spelled -- a command line, a save file, a level list, a
    /// [`load`](crate::Command::load) out of a tick -- is already text, and a
    /// typed reference meant each of those spellings had to be parsed back into
    /// it before anything could be compared.
    ///
    /// What that gives up is a compiler-checked level name, and it is worth
    /// being plain that it is given up: a game that mistypes one finds out when
    /// [`load`](Self::load) answers an error rather than when it builds. The
    /// error is at the moment the level is asked for and names the string that
    /// was asked for, which is the report a player and a bug reporter can both
    /// act on.
    ///
    /// # This never runs inside a tick
    ///
    /// It runs on a loader thread, which is why it may block. What makes the
    /// result deterministic is not this function but *where it is applied*:
    /// every peer applies it at the same tick, because the
    /// [`load`](crate::Command::load) that asked for it came out of a tick every
    /// peer ran. The runtime holds the simulation at that tick until the level
    /// is in hand, and each machine sits there for a different number of
    /// milliseconds.
    ///
    /// So this may read a clock, walk a directory and take as long as it takes.
    /// What it may **not** do is answer differently on two machines given the
    /// same name and the same bytes. The level is hashed into the session --
    /// always, with no way to ask for it not to be -- so a loader that folded in
    /// a timestamp or an environment variable disagrees at the tick it is
    /// applied, with the name in the report. That is the difference between
    /// "your data is stale, here is which file" and a bisect.
    ///
    /// # Where the bytes come from is the game's
    ///
    /// Nothing is handed to this. A game that reads from disk opens the file, a
    /// game whose levels are `include_bytes!` matches on the name, and a game
    /// with a virtual filesystem reaches for its own. This crate defines what a
    /// simulation *is*, and a filesystem is not part of that -- so it names no
    /// filesystem type and depends on no crate that has one.
    ///
    /// # Errors
    ///
    /// [`Error`](Self::Error) for a level that cannot be read, whether because
    /// its bytes are absent or because they are there and are not what they
    /// claim to be. A peer that cannot load leaves the session rather than
    /// hanging it, which is what the runtime does with this.
    fn load(name: &str) -> Result<Self, Self::Error>;
}
