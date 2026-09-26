//! What a controller sees of the network, and what it may ask of it.
//!
//! Plain data, so a game's controller needs no network types: every frame
//! the runtime hands [`Updating`](crate::Updating) a [`NetView`] of where
//! this machine stands -- alone, gathering in a lobby, or playing a linked
//! session -- and a list to push [`NetRequest`]s into, which it acts on
//! after the frame. A runtime built without networking answers every
//! request with a [`note`](NetView::note) saying so.

use std::string::String;
use std::vec::Vec;

/// Where this machine stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Stage {
    /// Playing a session of its own.
    #[default]
    Alone,
    /// In a lobby, hosting or joined, the session of its own still running.
    Gathering,
    /// Playing the lobby's session with the other machines, in lockstep.
    Linked,
}

/// One machine in the lobby.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Member {
    /// What it calls itself.
    pub name: String,
    /// The seat it plays.
    pub seat: u16,
    /// Whether it has said it is ready. The host always is.
    pub ready: bool,
    /// Whether it is the host.
    pub host: bool,
    /// Whether it is this machine.
    pub me: bool,
}

/// A lobby heard on the local network.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Heard {
    /// Where to join it, as `HOST:PORT`.
    pub address: String,
    /// Its host's name.
    pub name: String,
    /// How many seats are open.
    pub open: u16,
    /// Whether its session is already running: joining takes a seat a bot
    /// is playing.
    pub running: bool,
}

/// Where the network stands, for this frame.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "a handful of independent facts a menu reads, not a state machine in disguise"
)]
pub struct NetView {
    /// Whether this runtime can play other machines at all.
    pub online: bool,
    /// Where this machine stands.
    pub stage: Stage,
    /// Whether this machine hosts the lobby.
    pub hosting: bool,
    /// The port a hosted lobby is joined on.
    pub port: Option<u16>,
    /// How many seats the lobby's session has.
    pub seats: u16,
    /// Everyone in the lobby, the host first.
    pub members: Vec<Member>,
    /// Whether a host could start now: every seat taken, every guest ready.
    pub can_start: bool,
    /// Whether the lobbies on the local network are being listened for.
    pub browsing: bool,
    /// The lobbies heard lately.
    pub heard: Vec<Heard>,
    /// A line for a person to read about what just happened, if anything:
    /// why a join was turned away, that the host left.
    pub note: Option<String>,
}

/// What a controller may ask of the network. Acted on after the frame.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NetRequest {
    /// Host a lobby on `port` (0 for any) with `seats` seats, as `name`.
    Host {
        /// The port to be joined on.
        port: u16,
        /// How many seats the session has.
        seats: u16,
        /// What this machine calls itself.
        name: String,
    },
    /// Join the lobby at `address`, as `HOST:PORT`, as `name`; one in play is
    /// joined in progress.
    Join {
        /// Where the lobby is.
        address: String,
        /// What this machine calls itself.
        name: String,
    },
    /// Listen for lobbies on the local network, or stop.
    Browse(bool),
    /// Say whether this guest is ready.
    Ready(bool),
    /// Start the session, on the host, when everyone is ready.
    Start,
    /// Leave the lobby or the session and play alone again.
    Leave,
}
