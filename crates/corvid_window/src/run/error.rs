//! What opening a window, or the game behind it, refuses with.
//!
//! The seam is which half of the program stopped: everything here is either
//! the platform's answer or the host's, and nothing in it knows what an event
//! loop does.

/// The platform would not give us something to draw in.
///
/// Separate from [`Error`] and not generic, so that a caller which has already
/// dealt with its own half can carry this one without carrying its own error
/// type inside itself.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Opening {
    /// The platform would not give us an event loop, or the loop itself
    /// failed. On a machine with no display server this is what that looks
    /// like.
    #[error("this platform has no event loop for us: {0}")]
    Loop(#[source] winit::error::EventLoopError),
    /// The platform gave us a loop and would not open a window.
    #[error("this platform would not open a window: {0}")]
    Window(#[source] winit::error::OsError),
}

/// A window could not be opened, or a host stopped with a reason.
///
/// Exhaustive, unlike most of this workspace's error types, because the two
/// arms are the two halves of the program rather than a list of things that
/// can go wrong: anything else that ever fails is one side's or the other's.
/// A caller that has to tell them apart -- and the whole point of the split is
/// that it does -- should not have to write a wildcard for a third.
#[derive(Debug, thiserror::Error)]
pub enum Error<E: std::error::Error + 'static> {
    /// The platform's half.
    #[error(transparent)]
    Opening(#[from] Opening),
    /// The host stopped with a reason of its own.
    #[error("the game stopped: {0}")]
    Host(#[source] E),
}
