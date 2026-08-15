//! What a run refuses with.
//!
//! The seam is that nothing here is a game's tick going wrong. A tick cannot
//! fail -- it returns a state -- so every case is about the session the loop
//! was asked to play or about the filesystem it was asked to write to.

use std::{io, path::PathBuf};

use corvid_behavior::{PlayerId, SaveSlot};
use corvid_replay::{Refused, Shape};

use crate::cli::Argument;
use crate::saves::NotASave;

/// A run could not start, or could not be written down.
///
/// Nothing here is a game's tick going wrong. A tick cannot fail -- it returns a
/// state -- so every case below is about the session the loop was asked to play
/// or about the filesystem it was asked to write to.
///
/// Every variant writes a sentence rather than leaving a reader to `Debug` it,
/// and [`main`](crate::main) is what puts that sentence in front of an operator:
/// it hands **none** of these back, printing each to stderr and stopping the
/// process instead. A harness driving a run through [`launch`](crate::App::launch) or
/// [`run`](crate::App::run) gets them by value and does as it likes.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The command line [`launch`](crate::App::launch) read could not be acted on, or
    /// a `--level` this game could not open on.
    ///
    /// [`Argument::Help`] can be in here too, which is the one case that is not
    /// a failure: `--help` is a request for the usage, and it arrives as an
    /// error because the parser that noticed it may not print.
    ///
    /// [`main`](crate::main) answers this one apart from the rest: the usage to
    /// stdout and a zero status for a `Help`, and any other refusal to stderr
    /// with status 2 rather than the 1 a run that broke leaves -- so every one of
    /// these is one a harness driving a run through [`launch`](crate::App::launch) or
    /// [`arguments`](crate::App::arguments) asked for, and can match on.
    #[error(transparent)]
    Argument(Argument),
    /// No [`opening`](crate::App::opening) was given.
    #[error("this app has no opening, and nothing can invent a game's opening state for it")]
    Unopened,
    /// The seat this client would watch is not one the roster of the session
    /// being played has.
    ///
    /// A seat outside the roster is a seat with no camera in it, which is what
    /// makes this a refusal for a [`spectating`](crate::App::spectating) run as much
    /// as for a playing one. For a run that does play it, it is also a run that
    /// would record its actions nowhere, and a replay of it would be a replay
    /// of a session in which this client did nothing at all.
    ///
    /// The roster is the one the run plays with rather than the one the builder
    /// was handed: a [`load`](crate::App::load) or a [`replay`](crate::App::replay) discards
    /// the game's fresh opening and carries the saved session's roster on, so a
    /// seat is checked against that one.
    #[error(
        "this client watches seat {} and the roster has {seats}, so there would be nobody to \
         look through, and nowhere to record what it did if it played",
        seat.0
    )]
    Seat {
        /// The seat that was asked for.
        seat: PlayerId,
        /// How many the roster has.
        seats: usize,
    },
    /// A run asked for both [`bots`](crate::App::bots) and a
    /// [`transport`](crate::App::transport).
    ///
    /// **The bot is only asked on the path a run with nobody else in it takes.**
    /// A linked run submits this client's one action through
    /// [`Peer::submit`](corvid_lockstep::Peer::submit) and never calls the bot
    /// at all, so a run that accepted both would have taken a number of bots and
    /// played none of those seats. What is refused here is that silence: a flag
    /// that did nothing, on a run that looks from the outside like a run with
    /// bots in it.
    ///
    /// Making it mean something is real work rather than a missing branch. A
    /// bot's actions would have to reach the other machines, which means
    /// submitting for a seat that is not this client's and agreeing across the
    /// session on which peer is answering for it -- neither of which the wire
    /// says today.
    ///
    /// **A seat nobody is in still stalls a linked session**, which is what
    /// makes this worth stating rather than obvious: a column no peer writes
    /// pins the agreed frontier and every machine waits after
    /// [`Budget::ahead`](corvid_lockstep::Budget) ticks. Bots are not the
    /// answer to that, and would not be even if they were asked. What is, is a
    /// peer sitting in the seat.
    #[cfg(feature = "net")]
    #[error(
        "this run has {bots} bots and a transport, and a linked run never asks the bot -- so the \
         seats asked for would have been played by nobody"
    )]
    BotsAndPeers {
        /// How many were asked for.
        bots: u16,
    },
    /// The roster has no seats, so there is nobody to watch and no run.
    ///
    /// Separate from [`Seat`](Self::Seat) and checked before it, because the
    /// seat is not what is wrong: an empty roster has no seat to name and no
    /// camera to offer, so a spectator is refused by it exactly as a player is.
    #[error(
        "this session has no seats in its roster, so there is nobody to play and nobody to watch"
    )]
    NoSeats,
    /// The opening could not be made into a session.
    #[error("the opening is not a session: {0}")]
    Shape(#[source] Shape),
    /// The action log refused a write.
    ///
    /// The loop writes one entry per tick, at the frontier, into a row it has
    /// just grown, so this is [`Refused::Memory`] on a machine that has run out
    /// or a session whose log was replaced under the runtime by a caller
    /// holding the public field.
    #[error("the action log refused this tick's action: {0}")]
    Log(#[source] Refused),
    /// Two peers computed different states from what they agree is the same
    /// action log, and this run stopped rather than playing on.
    ///
    /// **This is a bug in the game, not a fault of the link.** A lost datagram
    /// is predicted through, a late one rolls back, and neither reaches here;
    /// what does is a `tick` that is not a pure function of the values its
    /// arguments denote -- a float whose rounding differs between the two
    /// machines, an iteration order that is a hash map's, a clock or an
    /// environment variable read from inside a simulation.
    ///
    /// The [`Desync`](corvid_lockstep::Desync) says which tick the digests
    /// differ at, which peer's mark disagreed, and how far back the two were
    /// last agreed -- and under `dev` a
    /// [`bisect`](corvid_lockstep::bisect) fills in which field diverged first.
    ///
    /// Boxed because it is much the largest thing this enum could carry and
    /// every other variant would pay for it by value.
    #[cfg(feature = "net")]
    #[error(
        "this session diverged: {0}; every peer simulated the same actions and did not reach \
         the same state, which is a tick that is not a pure function of what it was handed"
    )]
    Diverged(#[source] Box<corvid_lockstep::Desync>),
    /// The socket a `--listen`/`--connect` asked for could not be opened.
    ///
    /// A fact about the machine and the network rather than about the session:
    /// a port something else is already on, an address that resolves to
    /// nothing, a name no resolver knows. It is one variant naming which of the
    /// two halves failed, because "this port could not be bound" and "that
    /// address could not be reached" are the same kind of answer and the
    /// address is what a reader needs either way.
    #[cfg(feature = "net")]
    #[error("this run could not {what} {address}: {why}")]
    Socket {
        /// Which half: `bind` for the port here, `reach` for the machine there.
        what: &'static str,
        /// The address it was about, as it was written.
        address: String,
        /// Why not.
        #[source]
        why: io::Error,
    },
    /// A peer could not carry on for a reason that is not a divergence.
    ///
    /// A datagram naming a tick past the horizon -- the denial-of-service arm,
    /// since a tick number is the one thing in a session that arrives from
    /// somewhere else -- a peer that has sent two different actions for one
    /// tick, or a state offered for a tick outside the session.
    #[cfg(feature = "net")]
    #[error("this peer cannot carry on: {0}")]
    Halted(#[source] Box<corvid_lockstep::Halt>),
    /// A file could not be written.
    #[error("{} could not be written: {why}", path.display())]
    Wrote {
        /// Which file. `io::Error` does not carry the path it was about.
        path: PathBuf,
        /// Why not.
        #[source]
        why: io::Error,
    },
    /// A file could not be read.
    #[error("{} could not be read: {why}", path.display())]
    Read {
        /// Which file.
        path: PathBuf,
        /// Why not.
        #[source]
        why: io::Error,
    },
    /// A file is there and is not a save this build can play.
    #[error("{} is not a save this build can play: {why}", path.display())]
    Saved {
        /// Which file.
        path: PathBuf,
        /// Why not.
        #[source]
        why: NotASave,
    },
    /// The run was told to open a slot nothing has written.
    ///
    /// A refusal rather than a fresh game, because a run that was asked to
    /// resume and quietly started over is a run that has lost somebody's save.
    #[error(
        "nothing has been written to save slot {}, so there is nothing there to open",
        slot.0
    )]
    Empty {
        /// Which slot.
        slot: SaveSlot,
    },
    /// A device would not open, or stopped working.
    #[cfg(feature = "render")]
    #[error("the device could not draw this run: {0}")]
    Drew(#[source] corvid_render::Error),
    /// The platform would not give us an event loop or a window.
    ///
    /// On a machine with no display server, which is most build machines, this
    /// is what `window` answers.
    #[cfg(feature = "window")]
    #[error("this run has no window: {0}")]
    NoWindow(#[source] corvid_window::Opening),
    /// The player's binding file is there and cannot be used.
    ///
    /// A refusal rather than a fall back to the table the game ships, because
    /// the failure mode of falling back is a control that silently does
    /// nothing and a player with no way to learn why. What is wrong is a word
    /// in a text file, and the message names it.
    #[cfg(feature = "window")]
    #[error("{} is not a binding table this build can use: {why}", path.display())]
    Bound {
        /// Which file.
        path: PathBuf,
        /// What could not be read out of it.
        #[source]
        why: crate::controls::Misbound,
    },
    /// A windowed run ended without ever opening a window, so there is no
    /// session to hand back.
    ///
    /// The platform never resumed the application, which on a desktop means the
    /// loop was told to exit before it started.
    #[cfg(feature = "window")]
    #[error(
        "the event loop ended before the platform ever gave us a window, so this run played \
         no ticks"
    )]
    NeverOpened,
    /// The settings file is there and is not this game's settings.
    ///
    /// A refusal rather than a fall back to the defaults, for the reason
    /// [`Bound`](Self::Bound) is one: starting over would silently discard
    /// whatever the player had set, and what they would see is every control
    /// back where it started with nothing saying why.
    #[error("{} is not this game's settings: {why}", path.display())]
    Setting {
        /// Which file.
        path: PathBuf,
        /// What could not be read out of it.
        #[source]
        why: serde_json::Error,
    },
    /// Something could not be encoded on the way into a capture.
    #[error("{what} could not be encoded: {why}")]
    Encoded {
        /// What it was.
        what: &'static str,
        /// Why not.
        #[source]
        why: corvid_wire::Error,
    },
}
