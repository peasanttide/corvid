#![doc = include_str!("../README.md")]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the modules here are private, so pub(crate) and pub are equivalent — pub(crate) is the one that says what is meant, and it is what rustc's unreachable_pub asks for"
)]
// No `#![no_std]`, and no feature that would make one possible. This crate
// opens files and owns a thread. That is what makes `Handle<T>` un-hashable and
// therefore impossible to put in a `State`, which is the ring rule expressed as
// a type rather than as a paragraph.

mod handle;
mod load;
mod lod;
mod source;
mod store;

pub use handle::{Gone, Handle, Weak};
pub use load::Progress;
pub use lod::Lod;
// The filesystem moved below `std` into `corvid_files`, and is re-exported here
// so that a caller naming `corvid_asset::Source` still finds one. `Files` is
// the half that stayed, because it opens a directory.
pub use corvid_files::{Malformed, Memory, Missing, Source};
pub use source::Files;
pub use store::{Asset, Assets, Evicted, Unavailable};

/// How a game's level reference names a file.
///
/// Implemented on the [`Level`](corvid_behavior::Level) itself, which is where
/// the reference type is declared. There used to be an orphan-rule reason for
/// it to live on a marker instead; there is no marker now.
pub trait Locate: corvid_behavior::Level {
    /// The path this reference names, under whatever root the [`Source`] has.
    fn locate(reference: &Self::Reference) -> String;
}

/// Turn the name a tick emitted into the thing the client ring will read.
///
/// The bridge is one direction wide. `Command::load(reference)` reaches the
/// runtime, the runtime calls this, and the tick that named the `Ref` never
/// sees the [`Handle`]: a `Ref` is a small `Data` value the log carries and a
/// handle is a pointer into one machine's cache.
///
/// A game naming a *sub*-asset from inside a tick therefore names an index into
/// something the `Level` holds, which is the answer that was correct anyway —
/// an index is the same on every peer.
pub fn resolve<L: Locate + Asset>(assets: &Assets, reference: &L::Reference) -> Handle<L> {
    assets.load(&L::locate(reference))
}

/// The same bridge, waited on. What a barrier tick uses.
///
/// # Errors
///
/// [`Unavailable::Missing`] for a reference naming a path the source has
/// nothing under, and [`Unavailable::Malformed`] for bytes that will not
/// decode. A `Ref` naming a level that is not there fails the command rather
/// than stalling the barrier forever.
pub fn resolve_now<L: Locate + Asset>(
    assets: &Assets,
    reference: &L::Reference,
) -> Result<Handle<L>, Unavailable> {
    assets.load_now(&L::locate(reference))
}
