//! What a run hands back, and what it publishes while it is still running.
//!
//! The seam against `mod.rs` is direction: everything here comes *out* of a
//! run, and nothing in it is a setting.

use std::{fmt, sync::Arc};

use corvid_behavior::ExitCode;
use corvid_hash::Digest;
use corvid_replay::Session;
use corvid_time::Tick;

use crate::commands::Requests;
use crate::game::Game;

/// Where a run has got to, for whoever is watching it from another thread.
///
/// [`run`](crate::App::run) blocks the thread it was called on until the game stops,
/// so this is the only thing a supervisor, a progress bar or a test watchdog
/// has to look at while a run is in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Progress {
    /// The tick the runtime's current state is at.
    pub tick: Tick,
    /// The digest of that state.
    ///
    /// Always a mark rather than an [`Option`]: the loop pushes one for every
    /// tick it advances, so the trace can answer for [`tick`](Self::tick) in
    /// every run this crate drives. The one way it could not is a caller having
    /// replaced the session's trace, and the honest answer there is the digest
    /// of the state in hand rather than a `None` every reader has to branch on.
    pub mark: Digest,
    /// How many frames have been displayed.
    pub frames: u64,
    /// Whether the loop has stopped. The last value published before
    /// [`run`](crate::App::run) returns has this set, so a watcher can stop watching
    /// without a second channel to tell it to.
    pub finished: bool,
}

/// What a run leaves behind.
///
/// # Why the requests are here
///
/// A session, a state and an exit status are what a run is. The fourth field is
/// [`requests`](Self::requests), because the sink is not allowed to drop
/// anything silently and a record nobody can read is a record that does not
/// exist: an unhandled request would otherwise be a `tracing` warning and
/// nothing a test could assert on.
pub struct Outcome<G: Game> {
    /// The session the run played, which is everything needed to replay **what
    /// it still holds**.
    ///
    /// A run keeps a window of its own history by default and lets go of what
    /// is behind it, so this opens at the oldest tick the run kept rather than
    /// at the tick it started on: [`Session::first`](corvid_replay::Session::first)
    /// is where a replay of it begins and
    /// [`last`](corvid_replay::Session::last) is where the run stopped, which
    /// does not move. [`retain`](crate::App::retain) is where a run says it wants the
    /// lot, and [`capture`](crate::App::capture) already implies it.
    pub session: Session<G::State>,
    /// The state the run stopped at, which is the state at
    /// [`Session::last`](corvid_replay::Session::last).
    ///
    /// A handle rather than a value, because it is the handle the loop was
    /// holding: an [`Opening`](corvid_replay::Opening)'s origin and this speak the same type, so a run
    /// hands its last state over without copying it and a caller that wants it
    /// by itself derefs.
    pub state: Arc<G::State>,
    /// What the run asks the process to exit with. The status a
    /// [`quit`](corvid_behavior::Command::quit) named, or
    /// [`ExitCode::SUCCESS`] when the run stopped because
    /// [`until`](crate::App::until) said so.
    pub exit: ExitCode,
    /// Every request the ticks made, and what became of each.
    pub requests: Requests,
    /// What the netcode did over the whole run, for a run that had a
    /// [`transport`](crate::App::transport).
    ///
    /// Zeroed for a run with no other machines in it, which is the honest
    /// answer rather than an [`Option`]: a single-seat run heard nothing, sent
    /// nothing and rolled back never.
    #[cfg(feature = "net")]
    pub traffic: crate::Traffic,
}

/// The four fields, and the traffic when there is a peer.
///
/// Hand-written because a derive puts `G: Debug` on the impl, and a game is a
/// marker with nothing in it: the bound would be asking for a `Debug` that
/// says nothing in order to print fields that are not `G`.
impl<G: Game> fmt::Debug for Outcome<G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut printed = f.debug_struct("Outcome");
        let printed = printed
            .field("session", &self.session)
            .field("state", &self.state)
            .field("exit", &self.exit)
            .field("requests", &self.requests);
        #[cfg(feature = "net")]
        let printed = printed.field("traffic", &self.traffic);
        printed.finish()
    }
}
