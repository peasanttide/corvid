//! The builder, what a run hands back, and everything that can go wrong.

use corvid_control::Controller;
// Named for one call: the offscreen path builds the game's pipelines, and a
// build with no device never reaches it.
#[cfg(feature = "render")]
use corvid_render::Render;
use corvid_sound::Auralizer;
use std::{
    fmt, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use corvid_behavior::{ExitCode, Level, PlayerId, SaveSlot, State};
use corvid_hash::Digest;
use corvid_input::Input;
use corvid_replay::{LevelRef, Opening, Opens, Refused, Session, Shape};
use corvid_signal::Emitter;
use corvid_time::{Clock, Elapsed, Step, Tick, TickSpan};

use crate::{
    Arguments, Requests, Retention,
    capture::Capture,
    cli::{Argument, Load},
    game::{AuralizerConfig, BotConfig, ControllerConfig, Game, RenderConfig},
    headless::Headless,
    runtime::{Plan, Runtime},
    saves::{NotASave, Saves, StateAt},
    seating::Seating,
    settings::Settings,
};

/// The predicate [`App::until`] takes, named because it is written down three
/// times and because `Box<dyn Fn(&S, Tick) -> bool>` is
/// not a thing to read twice.
///
/// A newtype rather than the alias it used to be, so that the two structs
/// holding one can derive [`Debug`]. A closure has nothing to print, but
/// "there is a predicate here" is a fact about a run's settings worth keeping
/// in the line that prints them.
pub(crate) struct Stop<S>(Predicate<S>);

/// The boxed closure a [`Stop`] wraps, named so that the newtype's own field
/// is a word rather than the signature.
type Predicate<S> = Box<dyn Fn(&S, Tick) -> bool>;

impl<S> Stop<S> {
    /// Boxes a caller's predicate.
    pub(crate) fn new(predicate: impl Fn(&S, Tick) -> bool + 'static) -> Self {
        Self(Box::new(predicate))
    }

    /// Whether the run stops on `state` at `at`.
    pub(crate) fn reached(&self, state: &S, at: Tick) -> bool {
        (self.0)(state, at)
    }
}

impl<S> fmt::Debug for Stop<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Stop(<predicate>)")
    }
}

/// What [`App::open`] answers: the session the run plays, and — for a run
/// carrying a save or a recording on — the tick it opens at and the state
/// there.
///
/// An alias because the pair is a mouthful written out and the second half is
/// already a [`StateAt`]. A fresh session is [`None`] in the second position:
/// there is nothing to resume, and the opening's own origin is where it starts.
type Started<G> = (
    Session<<G as Game>::State>,
    Option<StateAt<<G as Game>::State>>,
);

/// Where a run has got to, for whoever is watching it from another thread.
///
/// [`run`](App::run) blocks the thread it was called on until the game stops,
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
    /// [`run`](App::run) returns has this set, so a watcher can stop watching
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
    /// does not move. [`retain`](App::retain) is where a run says it wants the
    /// lot, and [`capture`](App::capture) already implies it.
    pub session: Session<G::State>,
    /// The state the run stopped at, which is the state at
    /// [`Session::last`](corvid_replay::Session::last).
    ///
    /// A handle rather than a value, because it is the handle the loop was
    /// holding: an [`Opening`]'s origin and this speak the same type, so a run
    /// hands its last state over without copying it and a caller that wants it
    /// by itself derefs.
    pub state: Arc<G::State>,
    /// What the run asks the process to exit with. The status a
    /// [`quit`](corvid_behavior::Command::quit) named, or
    /// [`ExitCode::SUCCESS`] when the run stopped because
    /// [`until`](App::until) said so.
    pub exit: ExitCode,
    /// Every request the ticks made, and what became of each.
    pub requests: Requests<LevelRef<G::State>>,
    /// What the netcode did over the whole run, for a run that had a
    /// [`transport`](App::transport).
    ///
    /// Zeroed for a run with no other machines in it, which is the honest
    /// answer rather than an [`Option`]: a single-seat run heard nothing, sent
    /// nothing and rolled back never.
    #[cfg(feature = "net")]
    pub traffic: crate::Traffic,
}

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

/// The runtime, as a builder.
///
/// Nothing here runs until [`run`](Self::run), and everything before it is a
/// setting with a default. The defaults are the ones a headless run wants,
/// because a headless run is the kind that needs no setting up: a clock
/// [stepping](corvid_time::Clock::stepping) one period per call, the game's own
/// [`PERIOD`](Game::PERIOD), seat zero, an input snapshot with nothing
/// held, no capture, and [`Retention::RECENT`] — which is the one default that
/// reads another setting, since a run being captured keeps everything instead.
///
/// The one setting with no default is the [`opening`](Self::opening), because
/// nothing can invent a game's opening state for it.
///
/// # What printing one shows
///
/// Everything, including the opening. The four configs are
/// [`Data`](corvid_behavior::Data), which is already `Debug`; the clock and the
/// transport are trait objects whose traits say so; and the two boxed closures
/// are behind newtypes that name themselves. So this is a derive rather than a
/// hand-written impl that had to be kept in step with the fields above it.
///
/// What the derive asks for in exchange is `G: Debug`, which is what a derive
/// does with a type parameter: it bounds the parameter rather than the fields,
/// and none of the fields above is a `G`. A game is a marker with nothing in
/// it, so the bound is a `#[derive(Debug)]` on a unit struct — the same trade
/// [`Settings`] and `Screen` make, argued in the same terms.
///
/// The cost is that an opening prints a whole level and a whole state, which is
/// a long line for a game with a big one. That is the right way round: a
/// builder printing what it was actually given is what a `{:#?}` in a bug
/// report is for, and a caller who wants the short version prints the fields
/// they care about.
#[derive(Debug)]
pub struct App<G: Game> {
    /// What the player has set, which the runtime builds all four of the
    /// client-local halves from.
    ///
    /// Configs rather than a controller, a bot, a renderer and an ear, because
    /// only the runtime knows when the devices exist.
    ///
    /// [`None`] is "read the file", which is what [`run`](Self::run) does; a
    /// caller that set [`settings`](Self::settings) has overridden the file for
    /// this run and nothing is read.
    settings: Option<Settings<G>>,
    /// What the session starts from.
    opening: Option<Opening<G::State>>,
    /// Where real time comes from, or [`None`] to build the default from
    /// whatever [`rate`](Self::rate) ends up being.
    clock: Option<Box<dyn Elapsed>>,
    /// How often a tick runs.
    rate: TickSpan,
    /// Which seat this client watches, and whether it plays it.
    seating: Seating,
    /// How many of the roster's other seats the game's bot plays. See
    /// [`bots`](Self::bots).
    bots: u16,
    /// What carries this client's actions to the other machines, if there are
    /// any. See [`transport`](Self::transport).
    #[cfg(feature = "net")]
    transport: Option<Box<dyn corvid_net::Transport>>,
    /// How far ahead of the agreed frontier this client plays, and how far back
    /// it will roll. See [`budget`](Self::budget).
    #[cfg(feature = "net")]
    budget: corvid_lockstep::Budget,
    /// What the devices say. Nothing refills it; see [`input`](Self::input).
    input: Input,
    /// Where each frame's devices are read from, if a caller is standing in for
    /// a player. See [`inputs`](Self::inputs).
    /// Where to write the run down, if anywhere.
    capture: Option<PathBuf>,
    /// Where to write the session by itself, if anywhere. See
    /// [`record`](Self::record).
    record: Option<PathBuf>,
    /// How much of the session to keep, or [`None`] to let
    /// [`run`](Self::run) decide from whether the run is being written down.
    retention: Option<Retention>,
    /// What the operator asked for, applied by [`run`](Self::run) rather than
    /// when it was given. See [`arguments`](Self::arguments) for why.
    arguments: Option<Arguments>,
    /// Where this game's own directory is, or [`None`] for the default under
    /// the game's [`NAME`](State::NAME).
    state: Option<PathBuf>,
    /// The slot to open on, if the run is resuming one.
    load: Option<SaveSlot>,
    /// The recorded session to open on, if the run is carrying one on.
    replay: Option<PathBuf>,
    /// When to stop.
    stop: Option<Stop<G::State>>,
    /// How many ticks to run, if the caller said a number rather than a
    /// predicate. Turned into the tick to stop at in [`run`](Self::run), where
    /// the opening's first tick is known.
    ticks: Option<u64>,
    /// Where to publish progress, if anywhere.
    progress: Option<Emitter<Progress>>,
    /// Whether a window was asked for. What it says is
    /// [`NAME`](State::NAME), and what it shows is
    /// [`Render::icon`](corvid_render::Render::icon).
    #[cfg(feature = "window")]
    windowed: bool,
    /// How big to draw offscreen, if that was asked for.
    #[cfg(feature = "render")]
    offscreen: Option<corvid_render::Extent>,
    /// Which control means which action, if a game wrote a table.
    #[cfg(feature = "window")]
    bindings: Option<corvid_input::platform::Bindings>,
    /// Whether the event loop may run off the main thread.
    #[cfg(feature = "window")]
    any_thread: bool,
}

impl<G: Game> Default for App<G>
where
    ControllerConfig<G>: Default,
    BotConfig<G>: Default,
    RenderConfig<G>: Default,
    AuralizerConfig<G>: Default,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<G: Game> App<G>
where
    ControllerConfig<G>: Default,
    BotConfig<G>: Default,
    RenderConfig<G>: Default,
    AuralizerConfig<G>: Default,
{
    /// An app with every default and no opening.
    #[must_use]
    pub fn new() -> Self {
        Self {
            settings: None,
            opening: None,
            clock: None,
            rate: G::PERIOD,
            seating: Seating::default(),
            bots: 0,
            #[cfg(feature = "net")]
            transport: None,
            #[cfg(feature = "net")]
            budget: corvid_lockstep::Budget::DEFAULT,
            input: Input::new(&[]),
            capture: None,
            record: None,
            retention: None,
            arguments: None,
            state: None,
            load: None,
            replay: None,
            stop: None,
            ticks: None,
            progress: None,
            #[cfg(feature = "window")]
            windowed: false,
            #[cfg(feature = "render")]
            offscreen: None,
            #[cfg(feature = "window")]
            bindings: None,
            #[cfg(feature = "window")]
            any_thread: false,
        }
    }

    /// A run that depends on nothing about the machine it is on.
    ///
    /// Headless, on the game's own [`opening`](Self::opening), at its own
    /// [`PERIOD`](Game::PERIOD), with a state directory nothing else is using
    /// and [`Settings::default`] rather than whatever is in the player's file.
    /// It is the builder lines a test file would otherwise repeat, written
    /// once — and [`game!`](crate::game) generates the `app()` that calls it.
    ///
    /// # The directory, and why it is not the process's
    ///
    /// Under the system's temporary directory, named for the game's
    /// [`NAME`](State::NAME), the process, **and a counter this process
    /// keeps** — so every call gets one of its own. The process alone would not
    /// do it: several tests run concurrently in one binary, and two of them
    /// sharing a root is two runs sharing a save slot.
    ///
    /// Nothing is created here, and a headless run that saves nothing never
    /// creates it either — a run that does save leaves a directory behind,
    /// which is the cost of a constructor that has nowhere to hang a `Drop`. A
    /// test that wants the files cleaned up names its own directory with
    /// [`state`](Self::state).
    ///
    /// This is what a test builds from. A run in front of a player is
    /// [`new`](Self::new).
    #[must_use]
    pub fn sandbox() -> Self {
        // Shared by every instantiation of this function rather than one per
        // game, which is what a `static` in a generic function is and is all
        // that is wanted: what has to be unique is the directory, and the name
        // is in it already.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "corvid-sandbox-{}-{}-{unique}",
            <G::State as State>::NAME,
            std::process::id()
        ));
        Self::new()
            .opening(<G::State as Opens>::opening())
            .rate(G::PERIOD)
            .headless()
            .state(root)
            .settings(Settings::default())
    }

    /// Overrides what the player has set, for this run only.
    ///
    /// **Nothing needs this.** A run reads
    /// `$XDG_CONFIG_HOME/<NAME>/setting.json` and starts from what is in it, or
    /// from the defaults where there is no file — which is what a fresh install
    /// is. This is for the callers that cannot use that: a test that must not
    /// depend on the machine it runs on, a benchmark pinning a resolution, a
    /// tool driving a run with settings it was handed.
    ///
    /// It replaces the whole document rather than one field of it, because the
    /// four configs are one thing to a person and a run that took its controls
    /// from a caller and its volume from a file would be a run nobody can
    /// reproduce from either.
    ///
    /// The file is neither read nor written for a run that calls this: an
    /// override that persisted itself would be a test rewriting the developer's
    /// own settings.
    #[must_use]
    pub fn settings(mut self, settings: Settings<G>) -> Self {
        self.settings = Some(settings);
        self
    }

    /// Runs with no window, no adapter and no audio device.
    ///
    /// It undoes [`window`](Self::window) and
    /// [`offscreen`](Self::offscreen), and on an app that asked for neither it
    /// changes nothing, because a headless run is what [`new`](Self::new)
    /// already chose. `tests/headless.rs` asserts that adding it to a run
    /// changes neither the trace nor the outcome.
    /// `const` because undoing the two settings is two assignments now: they
    /// are a `bool` and an [`Option<Extent>`](corvid_render::Extent), and there
    /// is no longer a boxed painter to drop alongside them.
    #[must_use]
    pub const fn headless(mut self) -> Self {
        #[cfg(feature = "window")]
        {
            self.windowed = false;
        }
        #[cfg(feature = "render")]
        {
            self.offscreen = None;
        }
        self
    }

    /// Which control means which action.
    ///
    /// The default is `Bindings::placeholder` over the game's own declaration,
    /// and it is a placeholder in the strong sense: it binds by identifier
    /// number and has no idea what any action means. There is no per-device,
    /// rebindable table with glyphs in it here, and a game that wants one builds
    /// it itself.
    #[cfg(feature = "window")]
    #[must_use]
    pub fn bindings(mut self, bindings: corvid_input::platform::Bindings) -> Self {
        self.bindings = Some(bindings);
        self
    }

    /// Allows the event loop off the main thread, where the platform permits
    /// it.
    ///
    /// **A game leaves this alone.** X11 and Wayland are the only platforms
    /// that permit it and every other one ignores it, so a build that works
    /// this way is the one nobody ships. It is here because a test harness runs
    /// a test on a worker thread, and without it the only check that a window
    /// opens at all would be a person looking at one —
    /// `examples/hello/tests/windowed.rs` is the one caller in this workspace.
    #[cfg(feature = "window")]
    #[must_use]
    pub const fn any_thread(mut self, allowed: bool) -> Self {
        self.any_thread = allowed;
        self
    }

    /// Writes the run down under `directory`.
    ///
    /// The directory and its two subdirectories are created by
    /// [`run`](Self::run); an existing directory is written into rather than
    /// emptied. See the crate documentation for what a capture holds.
    #[must_use]
    pub fn capture(mut self, directory: impl Into<PathBuf>) -> Self {
        self.capture = Some(directory.into());
        self
    }

    /// Where this game keeps everything it keeps between runs.
    ///
    /// Under it are `saves/`, the settings file and — for a windowed run — the
    /// binding file. **One directory rather than three**: a player who copies a
    /// game to another machine copies one path, and a test that must not touch
    /// theirs redirects one call.
    ///
    /// The default is `$XDG_DATA_HOME/NAME/`, from the game's
    /// [`NAME`](State::NAME) — so `~/.local/share/NAME/` on a machine that has
    /// not set it, and `%APPDATA%\NAME\` on Windows. A player's files belong
    /// with the rest of their data rather than beside whatever directory the
    /// game was launched from. An environment that names no home at all falls
    /// back to `./NAME/`.
    #[must_use]
    pub fn state(mut self, directory: impl Into<PathBuf>) -> Self {
        self.state = Some(directory.into());
        self
    }

    /// Writes the session to `path` as the run plays.
    ///
    /// The file is what [`replay`](Self::replay) and `--demo` open, and it is
    /// the same bytes a [`capture`](Self::capture)'s `session` file holds — so
    /// a run recorded either way is a run either can carry on.
    ///
    /// It is written once, when the run ends, because a session is a whole
    /// thing rather than a stream: what a run holds at the end is what a replay
    /// of it needs. Like a capture, it implies [`Retention::Everything`] unless
    /// [`retain`](Self::retain) says otherwise, since a recording of the last
    /// few seconds of an hour is not the recording anybody asked for.
    ///
    /// The directory above the file is created if it is not there.
    #[must_use]
    pub fn record(mut self, path: impl Into<PathBuf>) -> Self {
        self.record = Some(path.into());
        self
    }

    /// Opens on a save slot rather than on the game's own opening.
    ///
    /// The run carries the saved session on from the tick it was written at:
    /// its log, its marks and its opening are the run's. A slot nothing has
    /// written is [`Error::Empty`], because a run that was asked to resume and
    /// silently started a new game would be a run that lost somebody's save.
    ///
    /// Reading is [`Session::seek`](corvid_replay::Session::seek), which is the
    /// same call rollback and time-walk are — so a save that cannot be replayed
    /// is refused here rather than a hundred ticks later.
    #[must_use]
    pub const fn load(mut self, slot: SaveSlot) -> Self {
        self.load = Some(slot);
        self
    }

    /// Opens on the session recorded in `path` rather than on the game's own
    /// opening.
    ///
    /// `path` is the `session` file a [`capture`](Self::capture) wrote. The run
    /// carries it on from its last tick, which is what makes a recorded run
    /// something to look at rather than something to take somebody's word for.
    ///
    /// Ignored by a run that also asked to [`load`](Self::load) a slot, which
    /// is the more specific of the two.
    #[must_use]
    pub fn replay(mut self, path: impl Into<PathBuf>) -> Self {
        self.replay = Some(path.into());
        self
    }

    /// How much of the session to keep in memory as the run plays.
    ///
    /// The default depends on whether anybody asked for the run to be written
    /// down, and this is the one setting in this builder where that is true. A
    /// run nobody is recording gets [`Retention::RECENT`], because a game left
    /// running for an hour accumulates 54 000 rows of actions and 54 000 digests
    /// that nothing has asked for. A run with a [`capture`](Self::capture) or a
    /// [`record`](Self::record) gets [`Retention::Everything`], because either
    /// is a request to write the run down and a recording of the last few
    /// seconds of an hour is not the thing that was asked for.
    ///
    /// Saying it here overrides both, in either direction and whatever order the
    /// two calls are made in: an unrecorded run can be told to keep everything,
    /// and a captured run can be told to keep a window — which records a capture
    /// of that window, and is the shape a long soak test with a bounded disk
    /// budget wants.
    ///
    /// # What a bounded run gives up
    ///
    /// Reach, and nothing else. The crate documentation has the table; the short
    /// of it is that save, replay, rollback and time-walk are one
    /// [`seek`](corvid_replay::Session::seek) over whatever the session still
    /// holds, so all four still work over the window and none of them reaches
    /// past it.
    #[must_use]
    pub const fn retain(mut self, retention: Retention) -> Self {
        self.retention = Some(retention);
        self
    }

    /// Where real time comes from.
    ///
    /// The default is [`Clock::stepping`](corvid_time::Clock::stepping) at the
    /// [`rate`](Self::rate)'s **own** period — a reading is one period, so a
    /// reading is one owed tick and the display sits on the endpoint state
    /// forever. It is built at [`run`](Self::run) so that setting the rate
    /// afterwards is not a trap, and `tests/headless.rs` pins it by running the
    /// default against an explicit `Clock::stepping(rate.period())` at a rate
    /// that is not the default one, where any other period owes a different
    /// number of ticks per reading. A run in front of a player passes
    /// [`Clock::wall`](corvid_time::Clock::wall) here, and that is the only way
    /// a wall clock enters this crate.
    #[must_use]
    pub fn clock(mut self, clock: impl Elapsed + 'static) -> Self {
        self.clock = Some(Box::new(clock));
        self
    }

    /// How often a tick runs.
    ///
    /// The default is [`G::PERIOD`](Game::PERIOD), which is the answer for
    /// every run a player plays: the period is a property of the session and
    /// two peers on different ones compute different states from the same
    /// actions. **A game never calls this.**
    ///
    /// It is still a setter because a harness may run a game at a rate the game
    /// did not choose — a soak test compressing an hour, a benchmark timing one
    /// tick — and a run like that has nobody on the other end of a link to
    /// disagree with.
    #[must_use]
    pub const fn rate(mut self, rate: TickSpan) -> Self {
        self.rate = rate;
        self
    }

    /// What the session starts from. The one setting with no default.
    #[must_use]
    pub fn opening(mut self, opening: Opening<G::State>) -> Self {
        self.opening = Some(opening);
        self
    }

    /// Which seat this client submits an action for, and looks through.
    ///
    /// The default is seat zero. Without a [`transport`](Self::transport) or
    /// any [`bots`](Self::bots) this is the only seat any action is recorded
    /// against and every other seat in the roster submits
    /// [`Action::default`](Default::default) forever, because nothing fills the
    /// other columns; a transport fills them from the machines sitting in them,
    /// and bots fill them from this process. A seat the roster does not have is
    /// [`Error::Seat`] whichever of the three it is.
    #[must_use]
    pub const fn seat(mut self, seat: PlayerId) -> Self {
        self.seating = Seating::Playing(seat);
        self
    }

    /// How many unclaimed seats the game's [`Bot`](crate::Game::Bot) plays.
    ///
    /// Bots take roster seats in order, skipping the seat this client plays. A
    /// spectator plays none, so it skips nothing:
    /// [`spectating`](Self::spectating) with `bots(2)` fills both seats of a
    /// two-seat game and the run is one this client only watches.
    ///
    /// Asking for more bots than there are seats fills the seats there are,
    /// because the number a caller wants and the number a roster has are two
    /// separate facts and the roster is the one that is true.
    ///
    /// # One bot, many seats
    ///
    /// There is a single [`Bot`](crate::Game::Bot) for the whole run, built
    /// from [`Settings::bot`](crate::Settings::bot), and it is asked once per
    /// seat per tick with [`Acting::seat`](corvid_control::Acting) naming which.
    /// A game whose bots differ from one another says so in that config, which
    /// is the game's own type; a runtime that built one instance per seat would
    /// be deciding for it that they are independent.
    ///
    /// # Not with a transport
    ///
    /// [`Error::BotsAndPeers`]. A seat filled locally is a seat every other
    /// machine in the session would have to fill identically, and a controller
    /// is not part of what a session records.
    #[must_use]
    pub const fn bots(mut self, count: u16) -> Self {
        self.bots = count;
        self
    }

    /// Watch a seat without playing it.
    ///
    /// The camera, the renderer and the ears are the watched seat's, and
    /// nothing is submitted for it: the column is filled by a peer or a bot, or
    /// holds the idle action. The controller is not asked for one either —
    /// [`action`](corvid_control::Controller::action) is not called at all on a
    /// run that plays nobody — so a spectator costs the run the whole of what
    /// deciding an action costs rather than only the write.
    ///
    /// The seat watched is whichever [`seat`](Self::seat) named, and the
    /// roster's first for a run that named none — so `--spectator --seat 1`
    /// watches the second seat without playing it. The two are one setting read
    /// twice: `seat` says *which*, and this says *whether*. Writing `seat`
    /// after this undoes it, because naming a seat to play is a claim on it.
    ///
    /// The seat is checked against the roster when the run opens, because that
    /// is when the roster is known: a `--load` or a [`replay`](Self::replay)
    /// plays the roster it resumed rather than the one the builder was handed.
    #[must_use]
    pub const fn spectating(mut self) -> Self {
        self.seating = Seating::Watching(self.seating.watched());
        self
    }

    /// Play against the peers this transport reaches.
    ///
    /// **A game implements nothing for this.** With a transport the loop owns a
    /// [`Peer`](corvid_lockstep::Peer): the action
    /// [`action`](corvid_control::Controller::action) returns is submitted for
    /// `now + delay` instead of being written straight into the log, whatever
    /// arrived is folded in — rolling back when a real action disagrees with
    /// what this machine predicted — and one datagram goes out per tick
    /// carrying this seat's newest actions and the digest of its state.
    /// `State` and `Present` are the same two implementations they were.
    ///
    /// Which seat this machine is is [`seat`](Self::seat), and
    /// `PeerId(n)` plays `PlayerId(n)`: two processes started by one command
    /// line have that, and a session assembled by a lobby is told otherwise
    /// over [`Channel::Control`](corvid_net::Channel).
    ///
    /// # What changes about a run
    ///
    /// The tick rate does not, the digest of a given action log does not, and
    /// the frames a client draws do not. What does is that the run's tick is
    /// the peer's: it may stall — [`Budget::ahead`](corvid_lockstep::Budget)
    /// past the tick every seat has confirmed, a peer waits rather than
    /// predicts further — and it may go backwards when a correction arrives,
    /// which is what a rollback is. A `--ticks N` therefore counts ticks the
    /// peer reached rather than iterations of the loop.
    ///
    /// A [`quit`](corvid_behavior::Command::quit) and a
    /// [`save`](corvid_behavior::Command::save) still reach the runtime, from
    /// the ticks simulated for the first time.
    /// [`Peer::advance`](corvid_lockstep::Peer::advance) carries
    /// the rule and what it costs.
    #[cfg(feature = "net")]
    #[must_use]
    pub fn transport(mut self, transport: Box<dyn corvid_net::Transport>) -> Self {
        self.transport = Some(transport);
        self
    }

    /// How much prediction this client is willing to do, for a run with a
    /// [`transport`](Self::transport).
    ///
    /// The default is [`Budget::DEFAULT`](corvid_lockstep::Budget): two ticks of
    /// input delay, six of rollback, eight ahead. It is a property of the
    /// machine and the link rather than of the session — two peers with
    /// different budgets compute the same states, because a budget decides when
    /// a peer waits and how much it re-simulates at once and never what a tick
    /// produces.
    ///
    /// Ignored by a run with no transport, which predicts nothing.
    #[cfg(feature = "net")]
    #[must_use]
    pub const fn budget(mut self, budget: corvid_lockstep::Budget) -> Self {
        self.budget = budget;
        self
    }

    /// What the devices say.
    ///
    /// Handed to [`action`](corvid_control::Controller::action) and
    /// [`look`](corvid_control::Controller::look) on every call, unchanged, for
    /// the whole run. **Nothing refills it.** There is no device layer here —
    /// nothing binds, notices a controller arriving, or rebinds — so a run
    /// either plays with the snapshot given here or plays with nothing held,
    /// and neither is a person at a keyboard.
    #[must_use]
    pub fn input(mut self, input: Input) -> Self {
        self.input = input;
        self
    }

    /// Publishes [`Progress`] into `emitter` after every tick, and once more
    /// with [`finished`](Progress::finished) set before [`run`](Self::run)
    /// returns.
    ///
    /// A publication is a lock and an allocation, so this costs a run one of
    /// each per tick. Leave it unset when nothing is watching; an app with no
    /// emitter publishes nothing and pays nothing.
    #[must_use]
    pub fn progress(mut self, emitter: Emitter<Progress>) -> Self {
        self.progress = Some(emitter);
        self
    }

    /// Stops the run at the first tick whose state satisfies `stop`.
    ///
    /// The predicate is handed the state a tick produced **and the tick that
    /// state is at**, so a run of a fixed length does not need the game to
    /// count. A game that counted for it would carry the counter in its `State`,
    /// which is hashed, serialized and sent — a column existing for a test's
    /// benefit and paid for on the wire. The tick the runtime already knows is
    /// the tick the predicate gets, and [`for_ticks`](Self::for_ticks) is the
    /// same thing written once.
    ///
    /// Checked against the state a tick produced, so a predicate that fires on
    /// the state at tick `N` stops the run with `N` ticks simulated and nothing
    /// after them, the same boundary
    /// [`quit`](corvid_behavior::Command::quit) stops at. The tick handed over
    /// is that `N`: the state's own tick, one past the tick that produced it.
    ///
    /// An app with no `until` whose game never asks to quit does not return.
    /// Nothing here can decide that for a caller: a game that is meant to run
    /// until someone closes the window is the ordinary case, and a headless run
    /// has no window to close.
    #[must_use]
    pub fn until(mut self, stop: impl Fn(&G::State, Tick) -> bool + 'static) -> Self {
        self.stop = Some(Stop::new(stop));
        self
    }

    /// Stops the run once `ticks` ticks have been simulated.
    ///
    /// The common case of [`until`](Self::until), and the one that costs a game
    /// nothing: the count is the runtime's rather than a counter the game has
    /// to carry in its hashed state. Counted from the opening's
    /// [`first`](corvid_replay::Opening::first) tick, so a session that opens at
    /// tick five and is asked for ten ticks stops at fifteen, and the state the
    /// run leaves is the state at that tick.
    ///
    /// `for_ticks(0)` stops before the first tick, which is a run of no ticks
    /// rather than a run without end — the predicate is checked after each
    /// tick, so the zero case is answered by the loop's own bound rather than
    /// by the predicate, and [`Outcome::state`] is the opening state.
    #[must_use]
    pub const fn for_ticks(mut self, ticks: u64) -> Self {
        self.ticks = Some(ticks);
        self
    }

    /// Applies what somebody typed on the command line.
    ///
    /// The operator's word beats the builder's, **whichever order the two are
    /// written in**, because these are the settings that are about the machine
    /// the game is being run on rather than about the game: whether there is a
    /// display, how long to run for, whether to record it. An argument that was
    /// not given changes nothing, so a game keeps every default it set.
    ///
    /// That is why this is the one setter here that does not take effect where
    /// it is written. It records the arguments and [`run`](Self::run) applies
    /// them, after every other builder call has had its say — an ordinary
    /// setter would be overwritten by a `for_ticks` two lines further down, and
    /// a game's `main` would silently ignore `--ticks`. Saying it twice keeps
    /// the second, because two command lines is one command line and the later
    /// one is the one being asked for.
    ///
    /// [`launch`](Self::launch) is this and [`run`](Self::run) together, and is
    /// what a game's `main` normally calls. This is the seam for a game that
    /// wants to read the arguments itself — to answer `--help` on its own
    /// stdout, or to accept flags of its own alongside these.
    #[must_use]
    pub fn arguments(mut self, arguments: Arguments) -> Self {
        self.arguments = Some(arguments);
        self
    }

    /// The session this run plays, and where in it the run starts.
    ///
    /// A run that was told to resume plays the session it was handed rather
    /// than the one the opening would have started, and it opens at that
    /// session's last tick rather than at its first. A run that was told
    /// neither opens the game's own opening, and the second half of the answer
    /// is [`None`] — there is nothing to resume, and the opening's origin state
    /// is where it starts.
    ///
    /// [`load`](Self::load) beats [`replay`](Self::replay), because a slot is
    /// the more specific of the two.
    fn open(&mut self, opening: Opening<G::State>, saves: &Saves) -> Result<Started<G>, Error> {
        let schema = opening.schema;
        let resumed = match (self.load.take(), self.replay.take()) {
            (Some(slot), _) => saves
                .read::<G::State>(slot, schema)?
                .ok_or(Error::Empty { slot })?,
            (None, Some(path)) => crate::saves::recorded::<G::State>(&path, schema)?,
            (None, None) => return Ok((Session::new(opening).map_err(Error::Shape)?, None)),
        };
        let (session, state) = resumed;
        let at = session.last();
        Ok((session, Some((at, state))))
    }

    /// The builder calls [`arguments`](Self::arguments) stands for, made at the
    /// last possible moment.
    ///
    /// # Errors
    ///
    /// [`Error::Argument`] carrying [`Argument::NotALevel`] for a `--level`
    /// whose JSON is not this game's level reference, and [`Error::Unopened`]
    /// for a `--level` on an app that has no opening to name a level in.
    fn apply(mut self, arguments: Arguments) -> Result<Self, Error> {
        if arguments.headless {
            self = self.headless();
        }
        if let Some(ticks) = arguments.ticks {
            self = self.for_ticks(ticks.0);
        }
        if let Some(path) = arguments.record {
            self = self.record(path);
        }
        if let Some(directory) = arguments.state {
            self = self.state(directory);
        }
        // The seat first, so that `--spectator --seat 1` watches the seat it
        // was told to. `--seat 0` and a command line that says nothing are the
        // same value, so a builder that chose a seat keeps it either way —
        // seat zero is what both sides default to, and there is nothing in the
        // parsed arguments that could tell the two apart.
        if arguments.seat != PlayerId(0) {
            self = self.seat(arguments.seat);
        }
        if arguments.spectator {
            self = self.spectating();
        }
        if arguments.num_bots > 0 {
            self = self.bots(arguments.num_bots);
        }
        match arguments.load {
            Some(Load::Save(slot)) => self = self.load(slot),
            Some(Load::Demo(path)) => self = self.replay(path),
            Some(Load::Level(json)) => self = self.open_on(&json)?,
            None => {}
        }
        Ok(self)
    }

    /// Opens on the level `json` names rather than on the one the game's
    /// opening does.
    ///
    /// Both halves of the opening move: the
    /// [`level`](corvid_replay::Opening::level) reference a session records,
    /// and the [`content`](corvid_replay::Opening::content) a tick is handed.
    /// The second is what makes this a flag that opens on a level rather than
    /// one that renames the level a run is already on — the reference is hashed
    /// into nothing, so a `--level` that moved only it would change what the
    /// session claims and not a byte of what it plays.
    ///
    /// # The files it reads, which are none
    ///
    /// [`Level::load`](corvid_behavior::Level::load) is handed a
    /// [`Source`](corvid_files::Source), and the one handed here is the empty
    /// one: this crate has no files of a game's, and inventing a directory to
    /// look in would be inventing where a game keeps its levels.
    ///
    /// So a game whose levels are self-describing — an enum, a name, anything
    /// its `load` builds without reading — opens on the one named, content and
    /// all. A game that reads its levels from files is **refused**, with what
    /// its own loader said about the missing file. That is the honest pair of
    /// answers: the alternative is a flag that appears to choose and does not.
    ///
    /// # Why JSON
    ///
    /// The value is JSON of the game's own
    /// [`Reference`](corvid_behavior::Level::Reference) rather than the
    /// [`FromStr`](core::str::FromStr) that type also has, because a
    /// `FromStr::Err` has no [`Display`](fmt::Display) bound on it and a
    /// refusal nobody can print is not a refusal.
    ///
    /// # Errors
    ///
    /// [`Error::Argument`] carrying [`Argument::NotALevel`] if the JSON is not
    /// this game's reference and [`Argument::UnreadableLevel`] if it is one and
    /// the level will not load from nothing, and [`Error::Unopened`] if there
    /// is no opening to name a level in.
    fn open_on(mut self, json: &str) -> Result<Self, Error> {
        let level: LevelRef<G::State> = serde_json::from_str(json).map_err(|why| {
            Error::Argument(Argument::NotALevel {
                value: json.to_owned(),
                why: why.to_string(),
            })
        })?;
        // `&()` is the source with nothing in it, which `corvid_files`
        // implements for exactly this: a caller that has no files to offer says
        // so in the type rather than by handing over an empty directory.
        let content = <<G::State as State>::Level as Level>::load(&level, &()).map_err(|why| {
            Error::Argument(Argument::UnreadableLevel {
                value: json.to_owned(),
                why: why.to_string(),
            })
        })?;
        let opening = self.opening.as_mut().ok_or(Error::Unopened)?;
        opening.level = level;
        opening.content = Arc::new(content);
        Ok(self)
    }

    /// Reads the standard arguments and plays the game.
    ///
    /// [`main`](crate::main) is what a game writes and this is the same reading
    /// of the command line for a harness that has already built an [`App`] of
    /// its own — one with a clock it chose, a seat it chose, or a stop
    /// predicate no flag can express — and wants the operator's word applied on
    /// top of it.
    ///
    /// ```no_run
    /// # use serde::{Deserialize, Serialize};
    /// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    /// # struct Nowhere;
    /// # impl corvid_behavior::Level for Nowhere {
    /// #     type Reference = String;
    /// #     fn load(_: &String, _: &dyn corvid_files::Source)
    /// #         -> Result<Self, corvid_files::Malformed> { Ok(Self) }
    /// # }
    /// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    /// # struct Bounce;
    /// # impl corvid_behavior::State for Bounce { /* … */
    /// #     const NAME: &'static str = "bounce";
    /// #     type Level = Nowhere; type Rules = (); type Action = ();
    /// # }
    /// # impl corvid_replay::Opens for Bounce {
    /// #     fn opening() -> corvid_replay::Opening<Self> { unimplemented!() }
    /// # }
    /// use corvid_replay::Opens;
    ///
    /// /// The game: a state, and nobody playing, drawing or listening.
    /// #[derive(Debug)]
    /// struct Hello;
    ///
    /// impl corvid_app::Game for Hello {
    ///     const PERIOD: corvid_time::TickSpan = corvid_time::TickSpan::CRADLE;
    ///     type State = Bounce;
    ///     type Controller = ();
    ///     type Bot = ();
    ///     type Render = ();
    ///     type Auralizer = ();
    /// }
    ///
    /// fn main() -> corvid_app::Result {
    ///     corvid_app::App::<Hello>::new()
    ///         .opening(Bounce::opening())
    ///         .launch()?;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// [`Arguments`] is the list and says why it is that short. A caller that
    /// wants none of it calls [`run`](Self::run) and the command line is never
    /// read.
    ///
    /// The [`Outcome`] is handed back rather than swallowed, so a caller that
    /// wants to report a digest, or to exit with the status a
    /// [`quit`](corvid_behavior::Command::quit) named, has it.
    ///
    /// # Errors
    ///
    /// [`Error::Argument`] for anything the command line could not be read as —
    /// including `--help`, which is not a failure and arrives as one because
    /// this crate may not print — and then whatever [`run`](Self::run) reports.
    pub fn launch(self) -> Result<Outcome<G>, Error> {
        let arguments = Arguments::from_env().map_err(Error::Argument)?;
        self.arguments(arguments).run()
    }

    /// Plays the game.
    ///
    /// # One bound, and what it buys
    ///
    /// `G: Game`, which carries the five types a game is: a state, a
    /// controller, a bot, a renderer and an ear. A game that reaches a run
    /// therefore has a `draw` whether it draws anything or not, and
    /// `type Render = ();` is the one line that says it draws nothing.
    ///
    /// What that buys is below the surface: this can name `Screen<G>`, which
    /// holds a game's pipelines by value and calls `Render::draw` directly, so
    /// nothing between the loop and the game is boxed, dispatched through a
    /// vtable, or reached through a function pointer.
    ///
    /// # Errors
    ///
    /// [`Error::Argument`] for a `--level` in the
    /// [`arguments`](Self::arguments) that this game cannot open on — the one
    /// flag whose value only the game can judge, and so the one that is refused
    /// here rather than by the parser. Then [`Error::Unopened`] if no opening
    /// was given, [`Error::Shape`] if the opening cannot be made into a
    /// session, [`Error::NoSeats`] if that session's roster is empty,
    /// [`Error::Seat`] if the seat is not in the roster of the session the run
    /// ends up playing, [`Error::BotsAndPeers`] if it asked for bots and a
    /// transport at once, [`Error::Log`] if the action log refuses a write, and
    /// [`Error::Wrote`] or [`Error::Encoded`] if a capture or a recording
    /// cannot be written. A run with a device adds [`Error::Drew`], and a
    /// windowed one adds [`Error::NoWindow`].
    pub fn run(mut self) -> Result<Outcome<G>, Error> {
        // The one setting applied here rather than where it was written, so
        // that an operator's flag beats a builder call made after it. Taken
        // rather than read, so that `apply`'s own builder calls cannot see it
        // and loop.
        if let Some(arguments) = self.arguments.take() {
            self = self.apply(arguments)?;
        }

        // Read before `prepare` below takes the transport, because it decides
        // which clock this run defaults to and the plan owns the transport from
        // there on.
        let networked = self.networked();
        // Refused rather than reconciled, and before anything is opened or read
        // — a run that is not going to happen should not have created a capture
        // directory on the way to saying so.
        #[cfg(feature = "net")]
        if networked && self.bots > 0 {
            return Err(Error::BotsAndPeers { bots: self.bots });
        }
        // The one directory this game keeps anything in, resolved once and
        // handed to everything that writes: the slots, the settings file and —
        // on the windowed path — the binding file. Three lookups would be three
        // answers to "where does this game write", and a `--state` that moved
        // some of them.
        let root = crate::saves::root(self.state.take(), <G::State as State>::NAME);
        // Either what a caller overrode or what the player has set. Read here
        // rather than in the builder, because reading a file is something a run
        // does and not something a `new` does.
        let settings = match self.settings.take() {
            Some(settings) => settings,
            None => Settings::load(&root)?,
        };
        let (plan, capture) = self.prepare(&root)?;

        #[cfg(feature = "window")]
        if self.windowed {
            return self.run_windowed(plan, capture, settings);
        }

        let clock = self.chosen_clock(networked);

        #[cfg(feature = "render")]
        if let Some(size) = self.offscreen {
            return self.run_offscreen(plan, capture, clock, size, settings);
        }

        self.run_headless(plan, capture, clock, settings)
    }

    /// Whether this run has another machine in it.
    ///
    /// Read while the transport is still on the builder — [`prepare`](Self::prepare)
    /// moves it into the [`Plan`] — because it is what
    /// [`clock`](Self::clock) defaults on.
    const fn networked(&self) -> bool {
        #[cfg(feature = "net")]
        {
            self.transport.is_some()
        }
        #[cfg(not(feature = "net"))]
        {
            false
        }
    }

    /// Everything a run needs that does not depend on which backend it gets:
    /// the session, the seat it is watched from, the capture, and the plan the
    /// runtime is driven by.
    ///
    /// # Errors
    ///
    /// [`Error::Unopened`], [`Error::Shape`], [`Error::NoSeats`],
    /// [`Error::Seat`], and whatever opening a capture directory or reading a
    /// save reported.
    fn prepare(&mut self, root: &Path) -> Result<(Plan<G::State>, Option<Capture>), Error> {
        let opening = self.opening.take().ok_or(Error::Unopened)?;
        let saves = Saves::under(root);

        let (session, resumed) = self.open(opening, &saves)?;

        // Against the roster that is actually in force, which on a `--load` or
        // a `--demo` is the resumed session's and not the fresh opening's —
        // the fresh one was thrown away by `open` above. Checking the discarded
        // roster would pass a seat of three into a two-seat save and fail a tick
        // later as `Error::Log`, at the write, with nothing saying which seat
        // was wrong.
        let seats = session.opening.roster.len();
        // First, because "seat zero is not one of the zero seats" is a true
        // thing to say and a useless one to read: what is wrong with an empty
        // roster is the roster, and it is wrong for a spectator exactly as much
        // as for a player.
        if seats == 0 {
            return Err(Error::NoSeats);
        }
        if usize::from(self.seating.watched().0) >= seats {
            return Err(Error::Seat {
                seat: self.seating.watched(),
                seats,
            });
        }

        // Roster order, skipping the seat this client submits for. A spectator
        // submits for nobody and so skips nothing, which is what lets bots fill
        // every seat of a run this client only watches. The same roster the
        // check above used, for the same reason: a `--load` plays the seats the
        // save has and not the ones the fresh opening described.
        let played = self.seating.playing();
        let bots: Vec<PlayerId> = (0..seats)
            .filter_map(|seat| u16::try_from(seat).ok().map(PlayerId))
            .filter(|seat| Some(*seat) != played)
            .take(usize::from(self.bots))
            .collect();

        let capture = self.capture.take().map(Capture::open).transpose()?;
        let record = self.record.take();

        // Counted from where the run opened, which is the opening's first tick
        // for a fresh session and the tick a save was written at for a resumed
        // one: `--ticks 10` is ten ticks of play either way.
        let opened = resumed
            .as_ref()
            .map_or_else(|| session.first(), |(at, _)| *at);
        let deadline = self.ticks.map(|ticks| Tick(opened.0.saturating_add(ticks)));

        // The one default that reads another setting. A run nobody is recording
        // keeps a window, and a run being written down keeps the lot — because
        // a capture of the last few seconds of an hour is not the recording
        // anybody asked for. `retain` overrides both.
        let retention = self.retention.unwrap_or_else(|| {
            if capture.is_some() || record.is_some() {
                Retention::Everything
            } else {
                Retention::default()
            }
        });
        let plan = Plan {
            session,
            seating: self.seating,
            bots,
            #[cfg(feature = "net")]
            transport: self.transport.take(),
            #[cfg(feature = "net")]
            budget: self.budget,
            input: self.input.clone(),
            stop: self.stop.take(),
            deadline,
            progress: self.progress.take(),
            retention,
            saves,
            root: root.to_path_buf(),
            record,
            resumed,
        };
        Ok((plan, capture))
    }

    /// The clock this run reads real time from, or the one that stands in for
    /// it — whichever [`clock`](Self::clock) was given, or the default below.
    ///
    /// The default is one *tick period* per reading and not one period of some
    /// other rate: a clock that stepped faster or slower than the rate it is
    /// paired with would owe the loop a number of ticks per reading that is not
    /// one, which is the whole of what makes a headless run a sequence of
    /// endpoint states. `tests/headless.rs` pins it against the substitution.
    fn chosen_clock(&mut self, networked: bool) -> Box<dyn Elapsed> {
        self.clock.take().unwrap_or_else(|| {
            // A run with other machines in it keeps real time, because they do.
            // A stepping clock is what makes a headless run a sequence of
            // endpoint states — one tick per reading, as fast as the processor
            // allows — and a peer pacing itself that way spends every tick it
            // is ahead of the session spinning against a frontier that only
            // moves when a *real* second has passed on somebody else's machine.
            // It converges either way; it converges having burned a core.
            if networked {
                return Box::new(corvid_time::Clock::wall()) as Box<dyn Elapsed>;
            }
            Box::new(Clock::stepping(self.rate.period()))
        })
    }

    /// A run with a window: the binding table is resolved, the platform is
    /// handed an event loop, and the loop runs inside it.
    ///
    /// # Errors
    ///
    /// [`Error::Bound`] for a binding file that cannot be used,
    /// [`Error::NoWindow`] if the platform would not give us one, and whatever
    /// the run itself reported.
    #[cfg(feature = "window")]
    fn run_windowed(
        mut self,
        plan: Plan<G::State>,
        capture: Option<Capture>,
        settings: Settings<G>,
    ) -> Result<Outcome<G>, Error> {
        // the snapshot from the window's devices every frame, and what a
        // binding table is written against is which actions exist.
        let declaration = plan.input.sets();
        if declaration.is_empty() {
            // Not an error, because a game with genuinely no actions is a
            // legal thing to run and refusing it here would be this call
            // deciding that for it. Said out loud, because the alternative
            // is a window that opens, draws, and answers every input query
            // with `RELEASED` for the rest of the run with nothing anywhere
            // pointing at the missing line.
            tracing::warn!(
                name: "corvid_app.undeclared",
                "this run declares no action sets, so the window binds no key \
                 and no axis and every input query will read released; a \
                 windowed run wants `App::input` for its declaration even \
                 though the values are refilled from the devices",
            );
        }
        // The table this run is bound by, in the order the three sources
        // beat each other: the game's own `Present::bindings` is the
        // author's answer, `App::bindings` is a harness overriding it for
        // one run, and the player's file beats both — which is what makes
        // it a rebinding rather than a suggestion.
        let shipped = self
            .bindings
            .take()
            .unwrap_or_else(|| corvid_input::platform::Bindings::placeholder(declaration));
        let bindings = crate::controls::resolve(&plan.root, declaration, shipped)?;
        let config = corvid_window::Config::new(<G::State as State>::NAME, declaration)
            .icon(None)
            .bindings(bindings)
            .any_thread(self.any_thread);
        let pending = crate::windowed::Pending::<G> {
            controls: settings.controls.clone(),
            bot: settings.bot.clone(),
            graphics: settings.graphics.clone(),
            audio: settings.audio.clone(),
            settings,
            plan,
            capture,
            rate: self.rate,
            // A window runs in front of a player, so the default clock is
            // the wall. A `Fake` here would run the simulation at whatever
            // rate the display asked for frames.
            clock: self
                .clock
                .take()
                .unwrap_or_else(|| Box::new(corvid_time::Clock::wall())),
        };
        let host = corvid_window::run(config, crate::windowed::Windowed::<G>::new(pending))
            .map_err(|why| match why {
                corvid_window::Error::Opening(opening) => Error::NoWindow(opening),
                corvid_window::Error::Host(why) => why,
            })?;
        host.into_outcome()
    }

    /// A run with an adapter and no window, which is what writes pictures on a
    /// build machine.
    ///
    /// # Errors
    ///
    /// [`Error::Drew`] if the device would not open, and whatever the run
    /// itself reported.
    #[cfg(feature = "render")]
    fn run_offscreen(
        self,
        plan: Plan<G::State>,
        capture: Option<Capture>,
        clock: Box<dyn Elapsed>,
        size: corvid_render::Extent,
        settings: Settings<G>,
    ) -> Result<Outcome<G>, Error> {
        let renderer = corvid_render::Renderer::offscreen(size).map_err(Error::Drew)?;
        // The pipelines are built here because here is where the device
        // is, which is the whole of what `Setup` used to be for.
        let graphics = G::Render::new(
            corvid_render::Opened {
                device: renderer.device(),
                queue: renderer.queue(),
                format: renderer.format(),
            },
            settings.graphics.clone(),
        );
        // `false`: this is the offscreen path, which has an adapter and no
        // window. Nobody is in front of it, so nothing opens a sound card.
        Runtime::new(
            plan,
            crate::screen::Screen::<G>::new(renderer, capture, false),
            G::Controller::new(settings.controls.clone()),
            G::Bot::new(settings.bot.clone()),
            Some(graphics),
            G::Auralizer::new(settings.audio.clone()),
            settings,
        )?
        .drive(clock, Step::new(self.rate))
    }

    /// A run with no device at all.
    ///
    /// # Errors
    ///
    /// Whatever the run reported.
    fn run_headless(
        self,
        plan: Plan<G::State>,
        capture: Option<Capture>,
        clock: Box<dyn Elapsed>,
        settings: Settings<G>,
    ) -> Result<Outcome<G>, Error> {
        // pipelines against and no renderer to hold. `None` is that, said
        // plainly, rather than a renderer built from a device that is not
        // there.
        drop(settings.graphics.clone());
        Runtime::new(
            plan,
            Headless::<G>::new(capture),
            G::Controller::new(settings.controls.clone()),
            G::Bot::new(settings.bot.clone()),
            Option::<G::Render>::None,
            G::Auralizer::new(settings.audio.clone()),
            settings,
        )?
        .drive(clock, Step::new(self.rate))
    }

    /// Opens a window and plays the game in it.
    ///
    /// The title is [`NAME`](State::NAME) and the icon is
    /// [`Render::icon`](corvid_render::Render::icon), because both are
    /// properties of the game rather than of this run: a game that spelled its
    /// name once here and once in the directory its saves land in would have
    /// two names.
    ///
    /// **The event loop takes the calling thread**, because on iOS, Android and
    /// the web it has to: the platform calls into the loop and a game that kept
    /// `main` would have nowhere to receive events. The loop is what calls this
    /// runtime, once per displayed frame.
    ///
    /// A windowed run refills its input snapshot from the window's devices, so
    /// the *values* [`input`](Self::input) carries are ignored — those are what
    /// a headless run plays with and nothing fills them there. Its
    /// **declaration** is not ignored, and is required: the table an
    /// [`action_sets!`](corvid_input::action_sets) generated is what
    /// the window's binding table is written against and what sizes the
    /// snapshot it refills, and this is the only place a run is told about it.
    /// Call `.input(Input::new(action::SETS))` for a windowed run even though
    /// its values will be replaced — the default is a snapshot over an empty
    /// table, which binds no key and no axis and answers
    /// [`RELEASED`](corvid_input::Digital::RELEASED) to every query
    /// for the whole run.
    ///
    /// The default clock becomes [`Clock::wall`](corvid_time::Clock::wall),
    /// because a window in front of a player runs in real time and the
    /// [`Clock::stepping`](corvid_time::Clock::stepping) a headless run defaults
    /// to would run the simulation as fast as the display asked for frames.
    ///
    /// # What this does not change
    ///
    /// The digest. The window fills an input snapshot and the game records
    /// `wgpu` calls, and neither of them is on the path from an action log to a
    /// state — `tests/windowless.rs` runs the same opening with a device and
    /// without one and compares the traces.
    ///
    /// `const`, and it is worth saying why that is possible: asking for a
    /// window is one `bool` now. It used to also build the game's drawing half
    /// and box it, because this was the only context that knew `G: Render` — and
    /// [`run`](Self::run) takes that bound itself now, so there is nothing to
    /// capture here.
    #[cfg(feature = "window")]
    #[must_use]
    pub const fn window(mut self) -> Self {
        self.windowed = true;
        self
    }

    /// Draws every frame into a texture `size` pixels across, with no window.
    ///
    /// The headless path with a real device on it: the same loop, the same
    /// `Render` implementation, the same trace, and an adapter actually
    /// rasterising it. It is what a build machine can run, it is the other
    /// half of the comparison that says a renderer cannot change a digest, and
    /// it is the only run that produces a picture — a capture's PNG is read
    /// back off this texture.
    ///
    /// Ignored on a run that also asked for a [`window`](Self::window), which
    /// has a target of its own.
    #[cfg(feature = "render")]
    #[must_use]
    pub const fn offscreen(mut self, size: corvid_render::Extent) -> Self {
        self.offscreen = Some(size);
        self
    }
}

/// A run could not start, or could not be written down.
///
/// Nothing here is a game's tick going wrong. A tick cannot fail — it returns a
/// state — so every case below is about the session the loop was asked to play
/// or about the filesystem it was asked to write to.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The command line [`launch`](App::launch) read could not be acted on, or
    /// a `--level` this game could not open on.
    ///
    /// [`Argument::Help`] can be in here too, which is the one case that is not
    /// a failure: `--help` is a request for the usage, and it arrives as an
    /// error because the parser that noticed it may not print.
    ///
    /// [`main`](crate::main) hands **none** of these back. It writes the usage
    /// to stdout for a `Help` and answers `Ok(())`, and writes any other
    /// refusal to stderr and stops the process with status 2 — so every one of
    /// these is one a harness driving a run through [`launch`](App::launch) or
    /// [`arguments`](App::arguments) asked for, and can match on.
    #[error(transparent)]
    Argument(Argument),
    /// No [`opening`](App::opening) was given.
    #[error("this app has no opening, and nothing can invent a game's opening state for it")]
    Unopened,
    /// The seat this client would watch is not one the roster of the session
    /// being played has.
    ///
    /// A seat outside the roster is a seat with no camera in it, which is what
    /// makes this a refusal for a [`spectating`](App::spectating) run as much
    /// as for a playing one. For a run that does play it, it is also a run that
    /// would record its actions nowhere, and a replay of it would be a replay
    /// of a session in which this client did nothing at all.
    ///
    /// The roster is the one the run plays with rather than the one the builder
    /// was handed: a [`load`](App::load) or a [`replay`](App::replay) discards
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
    /// A run asked for both [`bots`](App::bots) and a
    /// [`transport`](App::transport).
    ///
    /// A bot is a controller, and a controller is no part of what a session
    /// records: every peer would have to run the same one over the same
    /// settings to reach the same actions for the seats it filled, and nothing
    /// on the wire says which peer that is. So the combination is refused here
    /// rather than reconciled.
    ///
    /// **A seat nobody is in still stalls a linked session**, which is what
    /// makes this worth stating rather than obvious: a column no peer writes
    /// pins the agreed frontier and every machine waits after
    /// [`Budget::ahead`](corvid_lockstep::Budget) ticks. Bots are not the
    /// answer to that. What is, is a peer sitting in the seat.
    #[cfg(feature = "net")]
    #[error(
        "this run has {bots} bots and a transport, and every peer running its own bots would \
         write the same seats' columns from controllers that are not hashed"
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
    /// arguments denote — a float whose rounding differs between the two
    /// machines, an iteration order that is a hash map's, a clock or an
    /// environment variable read from inside a simulation.
    ///
    /// The [`Desync`](corvid_lockstep::Desync) says which tick the digests
    /// differ at, which peer's mark disagreed, and how far back the two were
    /// last agreed — and under `dev` a
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
    /// A datagram naming a tick past the horizon — the denial-of-service arm,
    /// since a tick number is the one thing in a session that arrives from
    /// somewhere else — a peer that has sent two different actions for one
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
