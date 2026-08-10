//! How far along a load is.

use corvid_fixed::Factor16;

/// A level being loaded, for a client half that wants to draw a bar.
///
/// # Client-local, and why the *fact* of loading is not
///
/// Never hashed, never sent, absent when nothing is loading -- so this is how
/// far along **one machine's** bytes are, and two peers loading the same level
/// hold different values of it at the same tick.
///
/// That is why the fact of loading does not live here. A game puts itself into
/// a loading state in the same tick that issues
/// [`Command::load`](crate::Command::load), and every peer agrees about that
/// because it came out of a deterministic tick. What no peer can agree about is
/// how far another machine's disk has got, which is exactly and only what this
/// carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Loading<'a> {
    /// Which level, by the name it was asked for with.
    pub name: &'a str,
    /// How much of it is in hand.
    ///
    /// [`ZERO`](Factor16::ZERO) for a load that has not started;
    /// [`ONE`](Factor16::ONE) is not reached, because a load that finished is a
    /// level rather than a `Loading`.
    pub progress: Factor16,
}

impl<'a> Loading<'a> {
    /// A load that has got this far.
    #[must_use]
    pub const fn new(name: &'a str, progress: Factor16) -> Self {
        Self { name, progress }
    }
}
