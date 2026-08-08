//! Where the session is, and how far along a load is.

use core::time::Duration;

use corvid_fixed::Factor16;
use corvid_time::Tick;

/// Where the session is: which tick, and how long it has been playing.
///
/// # What is deliberately not in here
///
/// **The interpolation weight.** That is
/// [`Render::draw`](../corvid_render/trait.Render.html#tymethod.draw)'s own
/// `alpha` argument, because it goes straight into a uniform and belongs beside
/// the call that writes it rather than inside a struct three other functions
/// also take.
///
/// So nothing on this struct is a `factor`. A field of that name sitting beside
/// `draw`'s argument of that name is how a shader ends up lerping with
/// something that is not a weight at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Time {
    /// Which tick the newest state is.
    pub tick: Tick,
    /// Wall clock since the session opened.
    ///
    /// Real time, so it moves at whatever rate the machine is running at and is
    /// not something a simulation may read. It is here because a client half
    /// smoothing a camera or fading a flash wants one, and nothing downstream
    /// of it is hashed.
    pub elapsed: Duration,
}

/// A level being loaded, for a client half that wants to draw a bar.
///
/// # Client-local, and why the *fact* of loading is not
///
/// Never hashed, never sent, absent when nothing is loading — so this is how
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
pub struct Loading<'a, R> {
    /// Which level.
    pub reference: &'a R,
    /// How much of it is in hand.
    ///
    /// [`ZERO`](Factor16::ZERO) for a load that has not started;
    /// [`ONE`](Factor16::ONE) is not reached, because a load that finished is a
    /// level rather than a `Loading`.
    pub progress: Factor16,
}

impl<'a, R> Loading<'a, R> {
    /// A load that has got this far.
    #[must_use]
    pub const fn new(reference: &'a R, progress: Factor16) -> Self {
        Self {
            reference,
            progress,
        }
    }
}
