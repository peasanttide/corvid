//! The one thing that can go wrong.

use crate::EmitterId;

/// What a [`System`](crate::System) refuses to do.
///
/// One variant, because there is one question this crate can answer no to. A
/// step, a burst count, a lifetime and a drag are all defined for every value
/// they can hold -- see [`Emitter`](crate::Emitter) on why the fields are public
/// and unvalidated -- so the only failure left is naming an emitter that is not
/// there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum ParticleError {
    /// The id names no live emitter here: it was removed, a later add has
    /// taken its slot over, or it was never an id of this system's at all and
    /// its index is past the end of the table.
    #[error("no emitter {} of generation {} in this system", .0.index, .0.generation)]
    UnknownEmitter(EmitterId),
}
