//! Where the session is: the pair a client half is handed once a frame.

use core::time::Duration;

use crate::Tick;

/// Where the session is: which tick, and how long it has been playing.
///
/// The two halves are deliberately unlike each other. [`tick`](Self::tick) is
/// the simulation's own count and is the same number on every peer;
/// [`elapsed`](Self::elapsed) is real time on one machine and is not. Putting
/// them in one value is what makes the difference visible at the point of use,
/// rather than leaving a caller to remember which of two arguments it may
/// compare against a neighbour's.
///
/// # What is deliberately not in here
///
/// **The interpolation weight.** That is the drawing call's own `alpha`
/// argument, because it goes straight into a uniform and belongs beside the
/// call that writes it rather than inside a struct three other functions also
/// take.
///
/// So nothing on this struct is a `factor`. A field of that name sitting beside
/// a drawing call's argument of that name is how a shader ends up lerping with
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
