//! Why a file this crate wrote will not read back as a session.
//!
//! The seam against `mod.rs` is that nothing here touches a filesystem: this
//! is what the decoder answers with, and every case is about the bytes rather
//! than about the file they came out of.

use corvid_hash::Digest;
use corvid_replay::Load;

/// A slot's bytes are not a save this build can play.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NotASave {
    /// The file is not a save, or the state inside it is not this game's.
    #[error("these are not the bytes of a save: {0}")]
    Bytes(#[source] corvid_wire::Error),
    /// The session inside it is not one this build can replay.
    #[error(transparent)]
    Session(#[from] Load),
    /// The session inside it does not reach its own last tick, which means the
    /// log and the opening in it disagree about where the session is.
    #[error("the session in this save does not reach its own last tick: {0}")]
    Unreachable(#[source] corvid_replay::Unreachable),
    /// Replaying the session produced a different state than the one saved
    /// beside it.
    ///
    /// The schema matched, so the two builds describe their types the same way
    /// and one of them computes something else out of them. That is the failure
    /// a schema digest cannot see, and it is worth refusing at the load rather
    /// than carrying into a session two peers will disagree about.
    #[error(
        "this save records the state at its last tick as {recorded} and replaying its own log \
         arrives at {replayed}: the build that wrote it describes its types exactly as this one \
         does and computes something else out of them"
    )]
    Diverged {
        /// The digest of the state the save recorded.
        recorded: Digest,
        /// The digest of the state replaying its log arrives at.
        replayed: Digest,
    },
}
