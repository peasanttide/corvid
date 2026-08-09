//! What a level is, and how one is read.

use corvid_files::{Malformed, Source};

use crate::Data;

/// Authored, immutable within a session, hashed into the opening.
///
/// Behind an [`Arc`](alloc::sync::Arc) at the call site, so switching levels is
/// a pointer swap and a snapshot ring does not hold a copy per tick.
pub trait Level: Data {
    /// How this game names a level.
    ///
    /// A type parameter rather than a Corvid enum, because only the game knows
    /// what its levels are: a game with a fixed set makes this an enum, one
    /// that loads from disk makes it a path, and one that names its levels by
    /// string makes it a `String`.
    ///
    /// [`FromStr`](core::str::FromStr) is required so that a command line, a
    /// save file and a level list all spell one the same way. It is what
    /// [`Command::load`](crate::Command::load) carries.
    type Reference: Data + core::str::FromStr;

    /// Read one.
    ///
    /// # This never runs inside a tick
    ///
    /// It runs on a loader thread, which is why it may block and why
    /// [`Source`] is synchronous. What makes the result deterministic is not
    /// this function but *where it is applied*: every peer applies it at the
    /// same tick, because the [`load`](crate::Command::load) that asked for it
    /// came out of a tick every peer ran. The runtime holds the simulation at
    /// that tick until the level is in hand, and each machine sits there for a
    /// different number of milliseconds.
    ///
    /// So this may read a clock, walk a directory and take as long as it takes.
    /// What it may **not** do is answer differently on two machines given the
    /// same reference and the same bytes. The level is hashed into the session
    /// -- always, with no way to ask for it not to be -- so a loader that folded
    /// in a timestamp or an environment variable disagrees at the tick it is
    /// applied, with the reference in the report. That is the difference
    /// between "your data is stale, here is which file" and a bisect.
    ///
    /// # Errors
    ///
    /// [`Malformed`] for a level that cannot be read, whether because its files
    /// are absent -- [`Missing`](corvid_files::Missing) converts -- or because
    /// they are there and are not what they claim to be. A peer that cannot
    /// load leaves the session rather than hanging it, which is what the
    /// runtime does with this.
    fn load(reference: &Self::Reference, files: &dyn Source) -> Result<Self, Malformed>;
}
