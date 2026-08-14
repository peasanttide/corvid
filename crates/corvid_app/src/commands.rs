//! What the runtime does with what a tick asked for.

use corvid_behavior::{ExitCode, SaveSlot, Scope};
use corvid_time::Tick;

/// What the runtime did with one request.
///
/// Four answers rather than one bit, because "the runtime has no code for this",
/// "the runtime ran its code and there was nothing there" and "the runtime ran
/// its code and the machine refused" are three different findings, and only the
/// first is a gap in this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Answer {
    /// The runtime acted on it.
    Done,
    /// The runtime looked and there was nothing to find: a
    /// [`Read`](Command::Read) of a slot nothing has ever written.
    Empty,
    /// The runtime had the code and the machine would not do it: a
    /// [`Save`](Command::Save) onto a full disk, or a directory that will not
    /// say what is in it.
    ///
    /// Not an error out of [`run`](crate::App::run), on purpose. A save that
    /// cannot be written is a fact about the filesystem rather than about the
    /// simulation, and aborting the tick for it would throw away the rest of the
    /// run -- including the capture, which is the one artefact that would let
    /// anybody see what the run had been doing. So the run carries on, the
    /// failure is reported at `ERROR` where it happened, and it is here so that
    /// a caller reading the [`Outcome`](crate::Outcome) finds it too.
    Failed,
    /// The runtime has no code for the request. It was recorded and a warning
    /// was emitted, and nothing else happened.
    ///
    /// Not an error and not a panic. A tick that asks for a rumble is a correct
    /// tick, and a runtime with no rumble in it is a runtime with a gap
    /// rather than a game with a bug -- so the request is kept, with the tick it
    /// was made on, and the run carries on.
    Unhandled,
}

/// One request a tick made, and what became of it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Request {
    /// The tick that asked. This is the tick whose
    /// [`tick`](corvid_behavior::State::tick) returned the command -- the
    /// state it produced is the one at `tick + 1`.
    pub tick: Tick,
    /// Whether the request is about the session or about this machine, as
    /// [`Command::scope`] classifies it. Recorded rather than recomputed
    /// because it is what the runtime routed on, and a record of a decision is
    /// worth more than a record of the input to it.
    pub scope: Scope,
    /// What became of it.
    pub answer: Answer,
    /// The request itself.
    pub command: Command,
}

/// Every request a run made, in the order the ticks made them.
///
/// A `Vec` and not a summary. The whole point of the sink is that nothing is
/// dropped silently, so what a run reports is the requests themselves, with the
/// ticks they were made on, and a caller counts whatever it wants to count.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Requests {
    /// Every request, in order.
    entries: Vec<Request>,
}

impl Requests {
    /// Every request, in the order the ticks made them.
    pub fn iter(&self) -> impl Iterator<Item = &Request> + '_ {
        self.entries.iter()
    }

    /// Only the ones the runtime had no code for.
    pub fn unhandled(&self) -> impl Iterator<Item = &Request> + '_ {
        self.entries
            .iter()
            .filter(|request| request.answer == Answer::Unhandled)
    }

    /// Only the ones the runtime tried and the machine refused.
    ///
    /// The other half of [`unhandled`](Self::unhandled), and the reason a run
    /// whose save would not write still hands back a session and a capture: the
    /// failure is here rather than in place of the run.
    pub fn failed(&self) -> impl Iterator<Item = &Request> + '_ {
        self.entries
            .iter()
            .filter(|request| request.answer == Answer::Failed)
    }

    /// How many requests were made.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the run made none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<'a> IntoIterator for &'a Requests {
    type Item = &'a Request;
    type IntoIter = std::slice::Iter<'a, Request>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

/// The runtime's end of [`Requests`]: the requests it acts on here, and the
/// record it keeps of the rest.
#[derive(Debug, Default)]
pub(crate) struct Sink {
    /// What is handed back in the [`Outcome`](crate::Outcome).
    requests: Requests,
    /// Set by the first [`Quit`](Command::Quit), which is what stops the loop.
    quit: Option<ExitCode>,
}

impl Sink {
    /// Takes one request, acts on it if the runtime knows how, and records it
    /// either way.
    ///
    /// `at` is the tick that asked, which is the tick whose `tick` returned the
    /// command rather than the tick of the state it produced.
    /// `answered` is what the loop already did with it, for the two requests
    /// the loop is the only thing that can act on: a save needs the session and
    /// the state, and neither is anything this type holds.
    pub(crate) fn absorb(&mut self, at: Tick, command: Command, answered: Option<Answer>) {
        let scope = command.scope();
        let answer = answered.unwrap_or_else(|| self.act(at, scope, &command));
        self.requests.entries.push(Request {
            tick: at,
            scope,
            answer,
            command,
        });
    }

    /// Whether a [`Quit`](Command::Quit) has been seen, and what it asked to
    /// exit with.
    pub(crate) const fn quit(&self) -> Option<ExitCode> {
        self.quit
    }

    /// The record, to be handed to the caller.
    pub(crate) fn into_requests(self) -> Requests {
        self.requests
    }

    /// Acts on the ones that need nothing but this record, warns about
    /// everything else.
    ///
    /// [`Save`](Command::Save) and [`Read`](Command::Read) are not here: both
    /// are about the session and the state, which the loop holds and this does
    /// not, so the loop acts on them and hands the answer to
    /// [`absorb`](Self::absorb).
    ///
    /// The routing is [`Command::scope`]'s and not a second classification of
    /// the variants: a global request is one the session makes and every peer
    /// has to agree about, and a local one belongs to this machine. The runtime
    /// runs one peer in one process, so both kinds are acted on here -- which is
    /// exactly why the scope is *recorded* rather than merely used.
    /// A `Quit` this peer agreed to alone reads no differently from one every
    /// peer agreed to until there is a second peer, and the record is what a
    /// lockstep runtime reconciles.
    fn act(&mut self, at: Tick, scope: Scope, command: &Command) -> Answer {
        match command {
            Command::Quit(code) => {
                // The first one wins. A tick that returns two `Quit`s has asked
                // to stop twice with two statuses, and the loop stops at the
                // first, so the status the process exits with is the first.
                if self.quit.is_none() {
                    self.quit = Some(*code);
                }
                Answer::Done
            }
            Command::Screenshot => Answer::Done,
            other => {
                // Never a panic and never a silent drop. A game that asks for
                // something this runtime cannot do yet keeps running, and the
                // gap is in the log and in the outcome rather than in a
                // stack trace.
                tracing::warn!(
                    name: "corvid_app.unhandled",
                    tick = %at,
                    scope = ?scope,
                    command = ?other,
                    "this runtime has no code for this request yet; it was recorded and not acted on",
                );
                Answer::Unhandled
            }
        }
    }
}

/// A sink that keeps what it was told, in order.
///
/// This is the runtime's own use of the shape `Command` was made a trait for:
/// a tick's requests have to be routed and recorded, so the loop wants them in
/// a list -- and what it passes is exactly what a test would pass.
///
/// A tick that asks for nothing leaves the `Vec` empty, and an empty `Vec` does
/// not allocate, so the usual tick pays nothing for this.
#[derive(Debug, Default)]
pub(crate) struct Asked(pub(crate) Vec<Command>);

impl corvid_behavior::Command for Asked {
    fn load(&mut self, name: &str) {
        self.0.push(Command::Load(name.to_owned()));
    }

    fn unload(&mut self, name: &str) {
        self.0.push(Command::Unload(name.to_owned()));
    }

    fn quit(&mut self, code: ExitCode) {
        self.0.push(Command::Quit(code));
    }

    fn save(&mut self, slot: SaveSlot) {
        self.0.push(Command::Save(slot));
    }

    fn read(&mut self, slot: SaveSlot) {
        self.0.push(Command::Read(slot));
    }

    fn screenshot(&mut self) {
        self.0.push(Command::Screenshot);
    }

    fn invite(&mut self, player: corvid_behavior::PlayerId) {
        self.0.push(Command::Invite(player));
    }

    fn join_lobby(&mut self, lobby: corvid_behavior::LobbyId) {
        self.0.push(Command::JoinLobby(lobby));
    }

    fn leave_lobby(&mut self) {
        self.0.push(Command::LeaveLobby);
    }

    fn set_presence(&mut self, presence: corvid_behavior::PresenceText) {
        self.0.push(Command::SetPresence(presence));
    }

    fn open_url(&mut self, url: corvid_behavior::Url) {
        self.0.push(Command::OpenUrl(url));
    }

    fn achieve(&mut self, achievement: corvid_behavior::AchievementId) {
        self.0.push(Command::Achieve(achievement));
    }

    fn stat(&mut self, id: corvid_behavior::StatId, value: i64) {
        self.0.push(Command::Stat { id, value });
    }
}

/// What a tick asked for, as a value the runtime can route and record.
///
/// The trait it comes from has one method per effect, which is right for the
/// call site -- a tick says `command.quit(code)` and means it. What a *record*
/// wants is one type it can put in a list, hand back in an `Outcome` and let a
/// caller match on, which is what this is.
///
/// So the enum did not disappear when `Command` became a trait; it stopped
/// being the thing a tick names, and became the thing a runtime keeps.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Command {
    /// Load a level, by the name [`Level::load`](corvid_behavior::Level::load)
    /// reads.
    Load(String),
    /// Drop one the simulation is finished with.
    Unload(String),
    /// Stop, with this status.
    Quit(ExitCode),
    /// Write a save.
    Save(SaveSlot),
    /// Ask whether there is a save in a slot.
    Read(SaveSlot),
    /// Capture the frame.
    Screenshot,
    /// Ask the platform's overlay to invite someone.
    Invite(corvid_behavior::PlayerId),
    /// Join a lobby.
    JoinLobby(corvid_behavior::LobbyId),
    /// Leave whichever lobby this peer is in.
    LeaveLobby,
    /// Set the line a friends list shows.
    SetPresence(corvid_behavior::PresenceText),
    /// Open a link outside the game.
    OpenUrl(corvid_behavior::Url),
    /// Award an achievement.
    Achieve(corvid_behavior::AchievementId),
    /// Set a tracked statistic.
    Stat {
        /// Which statistic.
        id: corvid_behavior::StatId,
        /// Its new value.
        value: i64,
    },
}

impl Command {
    /// Whether this request is about the session or about one machine.
    #[must_use]
    pub const fn scope(&self) -> Scope {
        match self {
            Self::Load(_)
            | Self::Unload(_)
            | Self::Quit(_)
            | Self::Save(_)
            | Self::Read(_)
            | Self::JoinLobby(_)
            | Self::LeaveLobby => Scope::Global,
            Self::Invite(_)
            | Self::SetPresence(_)
            | Self::OpenUrl(_)
            | Self::Screenshot
            | Self::Achieve(_)
            | Self::Stat { .. } => Scope::Local,
        }
    }
}
