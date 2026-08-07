//! The builder, what a run hands back, and everything that can go wrong.

use corvid_control::Controller;
use corvid_render::Render;
use corvid_sound::Auralizer;
use std::{fmt, io, path::PathBuf, sync::Arc};

use corvid_behavior::{ExitCode, PlayerId, SaveSlot, State};
use corvid_hash::Digest;
use corvid_input::Input;
use corvid_replay::{LevelRef, Opening, Refused, Session, Shape};
use corvid_signal::Emitter;
use corvid_time::{Clock, Fake, Step, Tick, TickRate};

use crate::{
    Arguments, Requests, Retention,
    arguments::Argument,
    capture::Capture,
    headless::Headless,
    runtime::{Plan, Runtime},
    saves::{NotASave, Saves, StateAt},
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
pub struct Outcome<S: State> {
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
    pub session: Session<S>,
    /// The state the run stopped at, which is the state at
    /// [`Session::last`](corvid_replay::Session::last).
    ///
    /// A handle rather than a value, because it is the handle the loop was
    /// holding: an [`Opening`]'s origin and this speak the same type, so a run
    /// hands its last state over without copying it and a caller that wants it
    /// by itself derefs.
    pub state: Arc<S>,
    /// What the run asks the process to exit with. The status a
    /// [`quit`](corvid_behavior::Command::quit) named, or
    /// [`ExitCode::SUCCESS`] when the run stopped because
    /// [`until`](App::until) said so.
    pub exit: ExitCode,
    /// Every request the ticks made, and what became of each.
    pub requests: Requests<LevelRef<S>>,
    /// What the netcode did over the whole run, for a run that had a
    /// [`transport`](App::transport).
    ///
    /// Zeroed for a run with no other machines in it, which is the honest
    /// answer rather than an [`Option`]: a single-seat run heard nothing, sent
    /// nothing and rolled back never.
    #[cfg(feature = "net")]
    pub traffic: crate::Played,
}

impl<S: State> fmt::Debug for Outcome<S> {
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
/// because a headless run is the kind that needs no setting up: a
/// [`Fake`](corvid_time::Fake) clock stepping one period per call, the
/// [`CRADLE`](TickRate::CRADLE) rate, seat zero, an input snapshot with nothing
/// held, no capture, and [`Retention::RECENT`] — which is the one default that
/// reads another setting, since a run being captured keeps everything instead.
///
/// The one setting with no default is the [`opening`](Self::opening), because
/// nothing can invent a game's opening state for it.
///
/// # What printing one shows
///
/// Everything, including the opening. The three configs are
/// [`Data`](corvid_behavior::Data), which is already `Debug`; the clock and the
/// transport are trait objects whose traits say so; and the two boxed closures
/// are behind newtypes that name themselves. So this is a derive rather than a
/// hand-written impl that had to be kept in step with the fields above it.
///
/// The cost is that an opening prints a whole level and a whole state, which is
/// a long line for a game with a big one. That is the right way round: a
/// builder printing what it was actually given is what a `{:#?}` in a bug
/// report is for, and a caller who wants the short version prints the fields
/// they care about.
#[derive(Debug)]
pub struct App<S: State, C = (), R = (), A = ()>
where
    C: Controller<S>,
    R: Render<S>,
    A: Auralizer<S>,
{
    /// What the player has set, which the runtime builds the controller from.
    ///
    /// A config rather than a controller, because only the runtime knows when
    /// the devices exist — and the same is true of the two below, for the
    /// device and the sound card.
    controls: C::Config,
    /// What the renderer is built from.
    graphics: R::Config,
    /// What the ear is built from.
    audio: A::Config,
    /// What the session starts from.
    opening: Option<Opening<S>>,
    /// Where real time comes from, or [`None`] to build the default from
    /// whatever [`rate`](Self::rate) ends up being.
    clock: Option<Box<dyn Clock>>,
    /// How often a tick runs.
    rate: TickRate,
    /// Which seat this client submits for.
    seat: PlayerId,
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
    feed: Option<crate::runtime::Feed>,
    /// Where to write the run down, if anywhere.
    capture: Option<PathBuf>,
    /// How much of the session to keep, or [`None`] to let
    /// [`run`](Self::run) decide from whether there is a capture.
    retention: Option<Retention>,
    /// What the operator asked for, applied by [`run`](Self::run) rather than
    /// when it was given. See [`arguments`](Self::arguments) for why.
    arguments: Option<Arguments>,
    /// Where the save slots live, or [`None`] for the default under the game's
    /// [`NAME`](State::NAME).
    saves: Option<PathBuf>,
    /// The slot to open on, if the run is resuming one.
    load: Option<SaveSlot>,
    /// The recorded session to open on, if the run is carrying one on.
    replay: Option<PathBuf>,
    /// When to stop.
    stop: Option<Stop<S>>,
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

impl<S: State, C: Controller<S>, R: Render<S>, A: Auralizer<S>> Default for App<S, C, R, A>
where
    C::Config: Default,
    R::Config: Default,
    A::Config: Default,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<S: State, C: Controller<S>, R: Render<S>, A: Auralizer<S>> App<S, C, R, A>
where
    C::Config: Default,
    R::Config: Default,
    A::Config: Default,
{
    /// An app with every default and no opening.
    #[must_use]
    pub fn new() -> Self {
        Self {
            controls: C::Config::default(),
            graphics: R::Config::default(),
            audio: A::Config::default(),
            opening: None,
            clock: None,
            rate: TickRate::CRADLE,
            seat: PlayerId(0),
            #[cfg(feature = "net")]
            transport: None,
            #[cfg(feature = "net")]
            budget: corvid_lockstep::Budget::DEFAULT,
            input: Input::new(&[]),
            feed: None,
            capture: None,
            retention: None,
            arguments: None,
            saves: None,
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

    /// What the player has set: the controller is built from this.
    ///
    /// A config rather than a controller, because only the loop knows when the
    /// devices exist. The same is true of [`graphics`](Self::graphics) and
    /// [`audio`](Self::audio) below.
    #[must_use]
    pub fn controls(mut self, config: C::Config) -> Self {
        self.controls = config;
        self
    }

    /// What the renderer is built from, once there is a device to build it
    /// against.
    #[must_use]
    pub fn graphics(mut self, config: R::Config) -> Self {
        self.graphics = config;
        self
    }

    /// What the ear is built from.
    #[must_use]
    pub fn audio(mut self, config: A::Config) -> Self {
        self.audio = config;
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

    /// Where the save slots live.
    ///
    /// The default is `./saves/NAME/`, from the game's
    /// [`NAME`](State::NAME), and it is a relative path on purpose: a
    /// relative default is one a test can point at and one that needs no
    /// dependency to compute, where a platform's application-data directory is
    /// neither. A game that wants that directory knows where it is on the
    /// machine it is running on, and says so here or with `--saves`.
    #[must_use]
    pub fn saves(mut self, directory: impl Into<PathBuf>) -> Self {
        self.saves = Some(directory.into());
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
    /// that nothing has asked for. A run with a [`capture`](Self::capture) gets
    /// [`Retention::Everything`], because a capture is a request to write the
    /// run down and a recording of the last few seconds of an hour is not the
    /// thing that was asked for.
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
    /// The default is [`Fake::stepping`](corvid_time::Fake::stepping) at the
    /// [`rate`](Self::rate)'s **own** period — a reading is one period, so a
    /// reading is one owed tick and the display sits on the endpoint state
    /// forever. It is built at [`run`](Self::run) so that setting the rate
    /// afterwards is not a trap, and `tests/headless.rs` pins it by running the
    /// default against an explicit `Fake::stepping(rate.period())` at a rate
    /// that is not the default one, where any other period owes a different
    /// number of ticks per reading. A run in front of a player passes
    /// [`Wall`](corvid_time::Wall) here, and that is the only way a wall clock
    /// enters this crate.
    #[must_use]
    pub fn clock(mut self, clock: impl Clock + 'static) -> Self {
        self.clock = Some(Box::new(clock));
        self
    }

    /// How often a tick runs.
    #[must_use]
    pub const fn rate(mut self, rate: TickRate) -> Self {
        self.rate = rate;
        self
    }

    /// What the session starts from. The one setting with no default.
    #[must_use]
    pub fn opening(mut self, opening: Opening<S>) -> Self {
        self.opening = Some(opening);
        self
    }

    /// Which seat this client submits an action for.
    ///
    /// The default is seat zero. Without a [`transport`](Self::transport) this
    /// is the only seat any action is recorded against and every other seat in
    /// the roster submits [`Action::default`](Default::default) forever,
    /// because nothing fills the other columns; with one, the other columns are
    /// filled by the machines sitting in them. A seat the roster does not have
    /// is [`Error::Seat`] either way.
    #[must_use]
    pub const fn seat(mut self, seat: PlayerId) -> Self {
        self.seat = seat;
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

    /// Where each frame's devices are read from, for a run with no devices.
    ///
    /// `source` is called once per reading of the clock, before the ticks that
    /// reading owes, and is handed the tick the run is about to simulate. What
    /// it answers is folded in exactly the way a window's reading is: levels
    /// replace, edges and displacements accumulate until a tick spends them, so
    /// a press that lands between two ticks reaches exactly one of them.
    ///
    /// This is the seam a **scripted** run needs, and there was no other one. A
    /// windowed run is refilled from the window once per displayed frame; a run
    /// without a window played the whole way through on the single snapshot
    /// [`input`](Self::input) was given, which is a player holding the same keys
    /// from the first tick to the last. So the things a person does — point at a
    /// button, press it, let go, press escape — could be written down against
    /// `look` and `intend` directly and not against a run.
    ///
    /// It replaces [`input`](Self::input)'s values from the first frame on. Its
    /// *declaration* still matters and is still read from there, for the reason
    /// a windowed run's is: the snapshot the loop folds into is sized from it.
    ///
    /// A windowed run ignores this, because the window is the device layer and
    /// two of them would be two answers about the same keyboard.
    #[must_use]
    pub fn inputs(mut self, source: impl FnMut(Tick) -> Input + 'static) -> Self {
        self.feed = Some(crate::runtime::Feed::new(source));
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
    pub fn until(mut self, stop: impl Fn(&S, Tick) -> bool + 'static) -> Self {
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
    fn open(
        &mut self,
        opening: Opening<S>,
        saves: &Saves,
    ) -> Result<(Session<S>, Option<StateAt<S>>), Error> {
        let schema = opening.schema;
        let resumed = match (self.load.take(), self.replay.take()) {
            (Some(slot), _) => saves
                .read::<S>(slot, schema)?
                .ok_or(Error::Empty { slot })?,
            (None, Some(path)) => crate::saves::recorded::<S>(&path, schema)?,
            (None, None) => return Ok((Session::new(opening).map_err(Error::Shape)?, None)),
        };
        let (session, state) = resumed;
        let at = session.last();
        Ok((session, Some((at, state))))
    }

    /// The builder calls [`arguments`](Self::arguments) stands for, made at the
    /// last possible moment.
    fn apply(mut self, arguments: Arguments) -> Self {
        if arguments.headless {
            self = self.headless();
        }
        if let Some(ticks) = arguments.ticks {
            self = self.for_ticks(ticks);
        }
        if let Some(directory) = arguments.capture {
            self = self.capture(directory);
        }
        if let Some(retention) = arguments.retain {
            self = self.retain(retention);
        }
        if let Some(directory) = arguments.saves {
            self = self.saves(directory);
        }
        if let Some(slot) = arguments.load {
            self = self.load(slot);
        }
        if let Some(path) = arguments.replay {
            self = self.replay(path);
        }
        self
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
    /// #     fn load(_: &String, _: &dyn corvid_behavior::Source)
    /// #         -> Result<Self, corvid_behavior::Malformed> { Ok(Self) }
    /// # }
    /// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    /// # struct Bounce;
    /// # mod hello { pub fn opening() -> corvid_replay::Opening<super::Bounce> { unimplemented!() } }
    /// # impl corvid_behavior::State for Bounce { /* … */
    /// #     const NAME: &'static str = "bounce";
    /// #     type Level = Nowhere; type Rules = (); type Action = ();
    /// # }
    /// fn main() -> corvid_app::Result {
    ///     corvid_app::App::<Bounce>::new()
    ///         .opening(hello::opening())
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
    pub fn launch(self) -> Result<Outcome<S>, Error> {
        let arguments = Arguments::from_env().map_err(Error::Argument)?;
        self.arguments(arguments).run()
    }

    /// Plays the game.
    ///
    /// # Four bounds, and what they buy
    ///
    /// `S: State`, `C: Controller<S>`, `R: Render<S>` and `A: Auralizer<S>` —
    /// a game is four types, and a run names every one of them. A game that
    /// reaches a run therefore has a `draw` whether it draws anything or not,
    /// and `type Graphics = ();` is the one line that says it draws nothing.
    /// There used to be a trait here reconciling two bounds under opposite
    /// `cfg`s, and a macro to write the implementation; there are four plain
    /// bounds now and one line.
    ///
    /// What that buys is below the surface: this can name `Screen<S>`, which
    /// holds a game's pipelines by value and calls `Render::draw` directly, so
    /// nothing between the loop and the game is boxed, dispatched through a
    /// vtable, or reached through a function pointer.
    ///
    /// # Errors
    ///
    /// [`Error::Unopened`] if no opening was given, [`Error::Shape`] if the
    /// opening cannot be made into a session, [`Error::Seat`] if the seat is not
    /// in the roster of the session the run ends up playing, [`Error::Log`] if
    /// the action log refuses a write, and [`Error::Wrote`] or
    /// [`Error::Encoded`] if a capture cannot be written. A run with a device
    /// adds [`Error::Drew`], and a windowed one adds [`Error::NoWindow`].
    #[allow(
        clippy::too_many_lines,
        reason = "one function choosing between four backends is the shape the choice has; splitting it would put each arm behind a name that says less than the arm does"
    )]
    pub fn run(mut self) -> Result<Outcome<S>, Error> {
        // The one setting applied here rather than where it was written, so
        // that an operator's flag beats a builder call made after it. Taken
        // rather than read, so that `apply`'s own builder calls cannot see it
        // and loop.
        if let Some(arguments) = self.arguments.take() {
            self = self.apply(arguments);
        }

        let opening = self.opening.take().ok_or(Error::Unopened)?;
        let saves = Saves::resolve(self.saves.take(), S::NAME);
        // Read before the plan below takes the transport, because it decides
        // which clock this run defaults to and the plan owns the transport from
        // there on.
        #[cfg(feature = "net")]
        let networked = self.transport.is_some();

        let (session, resumed) = self.open(opening, &saves)?;

        // Against the roster that is actually in force, which on a `--load` or
        // a `--replay` is the resumed session's and not the fresh opening's —
        // the fresh one was thrown away by `open` above. Checking the discarded
        // roster would pass a seat of three into a two-seat save and fail a tick
        // later as `Error::Log`, at the write, with nothing saying which seat
        // was wrong.
        let seats = session.opening.roster.len();
        if usize::from(self.seat.0) >= seats {
            return Err(Error::Seat {
                seat: self.seat,
                seats,
            });
        }

        let capture = self.capture.take().map(Capture::open).transpose()?;

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
            if capture.is_some() {
                Retention::Everything
            } else {
                Retention::default()
            }
        });
        let plan = Plan {
            session,
            seat: self.seat,
            #[cfg(feature = "net")]
            transport: self.transport.take(),
            #[cfg(feature = "net")]
            budget: self.budget,
            input: self.input.clone(),
            feed: self.feed.take(),
            stop: self.stop.take(),
            deadline,
            progress: self.progress.take(),
            retention,
            saves,
            resumed,
        };

        #[cfg(feature = "window")]
        if self.windowed {
            // The *declaration* rather than the values: a windowed run refills
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
            let bindings = crate::controls::resolve(plan.saves.root(), declaration, shipped)?;
            let config = corvid_window::Config::new(S::NAME, declaration)
                .icon(None)
                .bindings(bindings)
                .any_thread(self.any_thread);
            let pending = crate::windowed::Pending::<S, C, R, A> {
                controls: self.controls,
                graphics: self.graphics,
                audio: self.audio,
                plan,
                capture,
                rate: self.rate,
                // A window runs in front of a player, so the default clock is
                // the wall. A `Fake` here would run the simulation at whatever
                // rate the display asked for frames.
                clock: self
                    .clock
                    .take()
                    .unwrap_or_else(|| Box::new(corvid_time::Wall::new())),
            };
            let host = corvid_window::run(
                config,
                crate::windowed::Windowed::<S, C, R, A>::new(pending),
            )
            .map_err(|why| match why {
                corvid_window::Error::Opening(opening) => Error::NoWindow(opening),
                corvid_window::Error::Host(why) => why,
            })?;
            return host.into_outcome();
        }

        // The default is one *tick period* per reading and not one period of
        // some other rate: a clock that stepped faster or slower than the rate
        // it is paired with would owe the loop a number of ticks per reading
        // that is not one, which is the whole of what makes a headless run a
        // sequence of endpoint states. `tests/headless.rs` pins it against the
        // substitution.
        let clock = self.clock.take().unwrap_or_else(|| {
            // A run with other machines in it keeps real time, because they do.
            // The `Fake` below is what makes a headless run a sequence of
            // endpoint states — one tick per reading, as fast as the processor
            // allows — and a peer pacing itself that way spends every tick it
            // is ahead of the session spinning against a frontier that only
            // moves when a *real* second has passed on somebody else's machine.
            // It converges either way; it converges having burned a core.
            #[cfg(feature = "net")]
            if networked {
                return Box::new(corvid_time::Wall::new()) as Box<dyn Clock>;
            }
            Box::new(Fake::stepping(self.rate.period()))
        });
        #[cfg(feature = "render")]
        if let Some(size) = self.offscreen {
            let renderer = corvid_render::Renderer::offscreen(size).map_err(Error::Drew)?;
            // The pipelines are built here because here is where the device
            // is, which is the whole of what `Setup` used to be for.
            let graphics = R::new(
                renderer.device(),
                renderer.queue(),
                renderer.format(),
                self.graphics,
            );
            // `false`: this is the offscreen path, which has an adapter and no
            // window. Nobody is in front of it, so nothing opens a sound card.
            return Runtime::new(
                plan,
                crate::screen::Screen::new(renderer, capture, false),
                C::new(self.controls),
                Some(graphics),
                A::new(self.audio),
            )?
            .drive(clock, Step::new(self.rate));
        }

        // A headless run opens no device, so there is nothing to build
        // pipelines against and no renderer to hold. `None` is that, said
        // plainly, rather than a renderer built from a device that is not
        // there.
        drop(self.graphics);
        Runtime::new(
            plan,
            Headless::new(capture),
            C::new(self.controls),
            Option::<R>::None,
            A::new(self.audio),
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
    /// The default clock becomes [`Wall`](corvid_time::Wall), because a window
    /// in front of a player runs in real time and the
    /// [`Fake`](corvid_time::Fake) a headless run defaults to would run the
    /// simulation as fast as the display asked for frames.
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
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The command line [`launch`](App::launch) read could not be acted on.
    ///
    /// [`Argument::Help`] can be in here too, which is the one case that is not
    /// a failure: `--help` is a request for the usage, and it arrives as an
    /// error because the parser that noticed it may not print.
    /// [`main`](crate::main) never hands one back — it writes the usage to
    /// stdout and answers `Ok(())` — so a `Help` here is one a harness driving
    /// a run through [`launch`](App::launch) asked for.
    Argument(Argument),
    /// No [`opening`](App::opening) was given.
    Unopened,
    /// The [`seat`](App::seat) is not one the roster of the session being played
    /// has.
    ///
    /// A run with nobody in the seat it submits for would record its actions
    /// nowhere, and a replay of it would be a replay of a session in which this
    /// client did nothing at all. A roster with nobody in it lands here too,
    /// whatever the seat.
    ///
    /// The roster is the one the run plays with rather than the one the builder
    /// was handed: a [`load`](App::load) or a [`replay`](App::replay) discards
    /// the game's fresh opening and carries the saved session's roster on, so a
    /// seat is checked against that one.
    Seat {
        /// The seat that was asked for.
        seat: PlayerId,
        /// How many the roster has.
        seats: usize,
    },
    /// The opening could not be made into a session.
    Shape(Shape),
    /// The action log refused a write.
    ///
    /// The loop writes one entry per tick, at the frontier, into a row it has
    /// just grown, so this is [`Refused::Memory`] on a machine that has run out
    /// or a session whose log was replaced under the runtime by a caller
    /// holding the public field.
    Log(Refused),
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
    Diverged(Box<corvid_lockstep::Desync>),
    /// A peer could not carry on for a reason that is not a divergence.
    ///
    /// A datagram naming a tick past the horizon — the denial-of-service arm,
    /// since a tick number is the one thing in a session that arrives from
    /// somewhere else — a peer that has sent two different actions for one
    /// tick, or a state offered for a tick outside the session.
    #[cfg(feature = "net")]
    Halted(Box<corvid_lockstep::Halt>),
    /// A file could not be written.
    Wrote {
        /// Which file. `io::Error` does not carry the path it was about.
        path: PathBuf,
        /// Why not.
        why: io::Error,
    },
    /// A file could not be read.
    Read {
        /// Which file.
        path: PathBuf,
        /// Why not.
        why: io::Error,
    },
    /// A file is there and is not a save this build can play.
    Saved {
        /// Which file.
        path: PathBuf,
        /// Why not.
        why: NotASave,
    },
    /// The run was told to open a slot nothing has written.
    ///
    /// A refusal rather than a fresh game, because a run that was asked to
    /// resume and quietly started over is a run that has lost somebody's save.
    Empty {
        /// Which slot.
        slot: SaveSlot,
    },
    /// A device would not open, or stopped working.
    #[cfg(feature = "render")]
    Drew(corvid_render::Error),
    /// The platform would not give us an event loop or a window.
    ///
    /// On a machine with no display server, which is most build machines, this
    /// is what `window` answers.
    #[cfg(feature = "window")]
    NoWindow(corvid_window::Opening),
    /// The player's binding file is there and cannot be used.
    ///
    /// A refusal rather than a fall back to the table the game ships, because
    /// the failure mode of falling back is a control that silently does
    /// nothing and a player with no way to learn why. What is wrong is a word
    /// in a text file, and the message names it.
    #[cfg(feature = "window")]
    Bound {
        /// Which file.
        path: PathBuf,
        /// What could not be read out of it.
        why: crate::controls::Misbound,
    },
    /// A windowed run ended without ever opening a window, so there is no
    /// session to hand back.
    ///
    /// The platform never resumed the application, which on a desktop means the
    /// loop was told to exit before it started.
    #[cfg(feature = "window")]
    NeverOpened,
    /// Something could not be encoded on the way into a capture.
    Encoded {
        /// What it was.
        what: &'static str,
        /// Why not.
        why: corvid_wire::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Argument(why) => write!(f, "{why}"),
            Self::Unopened => f.write_str(
                "this app has no opening, and nothing can invent a game's opening state for it",
            ),
            Self::Seat { seat, seats } => write!(
                f,
                "this client submits for seat {} and the roster has {seats}, so \
                 there would be nowhere to record what it did",
                seat.0,
            ),
            Self::Shape(shape) => write!(f, "the opening is not a session: {shape}"),
            Self::Log(refused) => write!(f, "the action log refused this tick's action: {refused}"),
            #[cfg(feature = "net")]
            Self::Diverged(desync) => write!(
                f,
                "this session diverged: {desync}; every peer simulated the same actions                  and did not reach the same state, which is a tick that is not a pure                  function of what it was handed",
            ),
            #[cfg(feature = "net")]
            Self::Halted(halt) => write!(f, "this peer cannot carry on: {halt}"),
            Self::Wrote { path, why } => {
                write!(f, "{} could not be written: {why}", path.display())
            }
            Self::Read { path, why } => {
                write!(f, "{} could not be read: {why}", path.display())
            }
            Self::Saved { path, why } => {
                write!(
                    f,
                    "{} is not a save this build can play: {why}",
                    path.display()
                )
            }
            Self::Empty { slot } => write!(
                f,
                "nothing has been written to save slot {}, so there is nothing \
                 there to open",
                slot.0,
            ),
            Self::Encoded { what, why } => write!(f, "{what} could not be encoded: {why}"),
            #[cfg(feature = "render")]
            Self::Drew(why) => write!(f, "the device could not draw this run: {why}"),
            #[cfg(feature = "window")]
            Self::NoWindow(why) => write!(f, "this run has no window: {why}"),
            #[cfg(feature = "window")]
            Self::Bound { path, why } => write!(
                f,
                "{} is not a binding table this build can use: {why}",
                path.display(),
            ),
            #[cfg(feature = "window")]
            Self::NeverOpened => f.write_str(
                "the event loop ended before the platform ever gave us a window, so this run \
                 played no ticks",
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unopened | Self::Seat { .. } | Self::Empty { .. } => None,
            Self::Argument(why) => Some(why),
            Self::Shape(shape) => Some(shape),
            Self::Log(refused) => Some(refused),
            #[cfg(feature = "net")]
            Self::Diverged(desync) => Some(&**desync),
            #[cfg(feature = "net")]
            Self::Halted(halt) => Some(&**halt),
            Self::Wrote { why, .. } | Self::Read { why, .. } => Some(why),
            Self::Saved { why, .. } => Some(why),
            Self::Encoded { why, .. } => Some(why),
            #[cfg(feature = "render")]
            Self::Drew(why) => Some(why),
            #[cfg(feature = "window")]
            Self::NoWindow(why) => Some(why),
            #[cfg(feature = "window")]
            Self::Bound { why, .. } => Some(why),
            #[cfg(feature = "window")]
            Self::NeverOpened => None,
        }
    }
}
