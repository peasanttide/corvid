//! What the loop is handed, and the three small answers it works in.
//!
//! The seam against `mod.rs` is that nothing here is the loop: a [`Plan`] is
//! what a builder finished with, and the enums beside it are what one step of
//! the loop answers. None of them holds a backend or a device.

//! The loop: what happens per tick, what happens per displayed frame, and
//! where the boundary between them is.

use crate::commands::Command;
use std::{path::PathBuf, sync::Arc};

use corvid_behavior::{ExitCode, PlayerId};
use corvid_input::Input;
use corvid_replay::Session;
use corvid_signal::Emitter;
use corvid_time::Tick;

use crate::{
    Progress, Retention,
    app::Stop,
    game::Game,
    saves::{Saves, StateAt},
    seating::Seating,
};
use corvid_behavior::State;

/// Everything a run is, before it has a backend to display itself on.
///
/// One struct rather than seven arguments because there are three paths that
/// build a runtime -- headless, offscreen and windowed -- and the last of them
/// cannot build it until the platform hands over a window. Carrying the
/// ingredients as a value is what keeps the three from drifting: a setting added
/// here reaches all three or none.
pub(crate) struct Plan<S: State> {
    /// The session, already at its opening.
    pub(crate) session: Session<S>,
    /// Which seat this client looks through, and whether it plays it.
    pub(crate) seating: Seating,
    /// The seats the game's bot plays, in roster order.
    ///
    /// Empty for every run that did not ask for any, and that is the whole of
    /// what such a run pays: nothing is asked of the bot and nothing is
    /// written.
    pub(crate) bots: Vec<PlayerId>,
    /// The transport the other machines are behind, for a run that has any.
    ///
    /// [`None`] is one seat and no network, which is what a run that never
    /// calls [`App::transport`](crate::App::transport) plays.
    #[cfg(feature = "net")]
    pub(crate) transport: Option<Box<dyn corvid_net::Transport>>,
    /// How far ahead of the agreed frontier this machine will play, for a run
    /// with a transport.
    #[cfg(feature = "net")]
    pub(crate) budget: corvid_lockstep::Budget,
    /// What the devices say, for a run with no device layer under it.
    pub(crate) input: Input,
    /// When to stop, if the caller said.
    pub(crate) stop: Option<Stop<S>>,
    /// The tick to stop before, if the caller asked for a count.
    pub(crate) deadline: Option<Tick>,
    /// Where to publish progress, if anywhere.
    pub(crate) progress: Option<Emitter<Progress>>,
    /// How much of the session to keep as it is played.
    pub(crate) retention: Retention,
    /// Where a [`Save`](Command::Save) writes and a [`Read`](Command::Read)
    /// looks.
    pub(crate) saves: Saves,
    /// The one directory this game keeps anything in: the slots are under it,
    /// and so are the settings file and the binding file.
    pub(crate) root: PathBuf,
    /// Where to write the session when the run ends, if anywhere. See
    /// [`App::record`](crate::App::record).
    pub(crate) record: Option<PathBuf>,
    /// What the devices say, frame by frame, for a run whose caller is
    /// standing in for a player.
    /// The tick the run opens at and the state there, for a run that was handed
    /// a session rather than starting one.
    ///
    /// [`None`] is a fresh session, which opens at
    /// [`Session::first`](corvid_replay::Session::first) on the opening's own
    /// origin state. A `--load` or a `--demo` fills this in, because the
    /// session it hands over has already been played and the state at its last
    /// tick is what the run carries on from.
    pub(crate) resumed: Option<StateAt<S>>,
}

/// How far back the run can still reach, and the state it would reopen at.
///
/// [`Retention`] is the setting; this is what the loop does with it. The kept
/// state is what makes a bounded run possible at all: a session cannot forget
/// its first rows without being handed the state at the tick it is left opening
/// on, and the only place that state exists is the loop that produced it.
pub(super) enum Horizon<S: State> {
    /// Nothing is forgotten.
    Everything,
    /// A window, the tick the last state was set aside at, and that state.
    ///
    /// [`None`] until the run has been going for a whole window, which is why
    /// the first window's worth of ticks is never forgotten however small the
    /// window is: there is nothing yet to reopen at.
    Recent {
        /// How far back the run is sure to be able to reach.
        window: u64,
        /// The tick [`kept`](Self::Recent::kept) is the state at, or the tick
        /// the session opened on before there is one.
        marked: Tick,
        /// The state at [`marked`](Self::Recent::marked).
        kept: Option<Arc<S>>,
    },
}

/// Who is simulating: this machine alone, or this machine and the peers a
/// transport reaches.
///
/// The session is inside either arm rather than beside them, because a linked
/// run's session belongs to the [`Peer`](corvid_lockstep::Peer) -- a rollback
/// rewrites the action log and the mark trace, and a second owner of those
/// would be a second answer to what the session is.
pub(super) enum Play<S: State> {
    /// One seat, no network, and the loop writes its own action into the log
    /// and simulates it. Every run that names no transport.
    ///
    /// Boxed for the same reason the linked arm is: a `Session` is the larger
    /// half of this enum by two orders of magnitude, and a `Play` is moved
    /// once per builder step.
    Local(Box<Session<S>>),
    /// A peer that predicts, rolls back and exchanges digests, and the
    /// transport its datagrams ride on.
    #[cfg(feature = "net")]
    Linked(Box<crate::net::Link<S>>),
}

impl<S: State> Play<S> {
    /// The session being played.
    pub(super) fn session(&self) -> &Session<S> {
        match self {
            Self::Local(session) => session,
            #[cfg(feature = "net")]
            Self::Linked(link) => link.session(),
        }
    }

    /// The same, mutably, for the two things done to a session that are no part
    /// of simulating it: writing a save and forgetting the far past.
    pub(super) fn session_mut(&mut self) -> &mut Session<S> {
        match self {
            Self::Local(session) => session,
            #[cfg(feature = "net")]
            Self::Linked(link) => link.session_mut(),
        }
    }

    /// The session, once the run is over.
    pub(super) fn into_session(self) -> Session<S> {
        match self {
            Self::Local(session) => *session,
            #[cfg(feature = "net")]
            Self::Linked(link) => link.into_session(),
        }
    }
}

/// What one tick produces: the state after it, and everything it asked the
/// platform for.
///
/// An alias because it is written at both ends of the one call that produces
/// it, and the second half is a list of a game's own requests rather than
/// anything this crate has a shorter name for.
pub(super) type Ticked<G> = (<G as Game>::State, Vec<Command>);

/// Whether the loop carries on.
pub(super) enum Flow {
    /// Keep going.
    Go,
    /// Stop, with this status.
    Stop(ExitCode),
}
