//! The builder, what a run hands back, and everything that can go wrong.

use std::{
    fmt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use corvid_behavior::{SaveSlot, State};
use corvid_input::Input;
use corvid_replay::{Opening, Opens, Session};
use corvid_signal::Emitter;
use corvid_time::{Elapsed, Tick, TickSpan, Ticks};

use crate::{
    Arguments, Retention,
    game::{AuralizerConfig, BotConfig, ControllerConfig, Game, RenderConfig},
    saves::StateAt,
    seating::Seating,
    settings::Settings,
};

/// The predicate [`App::until`] takes, named because it is written down three
/// times and because `Box<dyn Fn(&S, Tick) -> bool>` is
/// not a thing to read twice.
///
/// A newtype rather than a bare alias, so that the two structs holding one can
/// derive [`Debug`]. A closure has nothing to print, but
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

/// The fact of a predicate rather than the predicate.
///
/// Hand-written because a closure has no `Debug` to derive one from, and a
/// derive would put `S: Debug` on the impl for a parameter no field holds.
impl<S> fmt::Debug for Stop<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Stop(<predicate>)")
    }
}

/// What [`App::open`] answers: the session the run plays, and -- for a run
/// carrying a save or a recording on -- the tick it opens at and the state
/// there.
///
/// An alias because the pair is a mouthful written out and the second half is
/// already a [`StateAt`]. A fresh session is [`None`] in the second position:
/// there is nothing to resume, and the opening's own origin is where it starts.
type Started<G> = (
    Session<<G as Game>::State>,
    Option<StateAt<<G as Game>::State>>,
);

mod backends;
mod error;
mod launch;
mod opening;
mod outcome;
mod session;
mod settings;

pub use error::Error;
pub use outcome::{Outcome, Progress};

/// The runtime, as a builder.
///
/// Nothing here runs until [`run`](Self::run), and everything before it is a
/// setting with a default. The defaults are the ones a headless run wants,
/// because a headless run is the kind that needs no setting up: a clock
/// [stepping](corvid_time::Clock::stepping) one period per call, the game's own
/// [`PERIOD`](Game::PERIOD), seat zero, an input snapshot with nothing
/// held, no capture, and [`Retention::RECENT`] -- which is the one default that
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
/// it, so the bound is a `#[derive(Debug)]` on a unit struct -- the same trade
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
    /// the game's [`NAME`](corvid_behavior::State::NAME).
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
    ticks: Option<Ticks>,
    /// Where to publish progress, if anywhere.
    progress: Option<Emitter<Progress>>,
    /// Whether a window was asked for. What it says is
    /// [`NAME`](corvid_behavior::State::NAME), and what it shows is
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

/// The same app [`new`](Self::new) builds.
///
/// Hand-written because a derive would ask every field for its own default,
/// and half of them -- the rate, the seating -- are the game's answer rather
/// than the field type's.
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
    /// once -- and [`game!`](crate::game) generates the `app()` that calls it.
    ///
    /// # The directory, and why it is not the process's
    ///
    /// Under the system's temporary directory, named for the game's
    /// [`NAME`](corvid_behavior::State::NAME), the process, **and a counter this process
    /// keeps** -- so every call gets one of its own. The process alone would not
    /// do it: several tests run concurrently in one binary, and two of them
    /// sharing a root is two runs sharing a save slot.
    ///
    /// # What is removed, and what is left behind
    ///
    /// **Whatever was there is removed here**, and that is the half of this
    /// that matters. The counter restarts at zero in every process, so the
    /// second run of a test binary on a machine that has recycled the process id
    /// resolves to the path the first one used -- and a run that opened a
    /// directory holding a previous run's `saves/` is a run that silently
    /// resumes somebody else's game. A sandbox is defined by depending on
    /// nothing about the machine it is on, and a leftover directory is exactly
    /// such a dependency.
    ///
    /// Nothing is *created* here, and a headless run that saves nothing never
    /// creates it either -- a run that does save leaves a directory behind,
    /// which is the cost of a constructor that has nowhere to hang a `Drop`.
    /// That litter is only litter, because the next call to reach the path
    /// clears it before reading anything. A test that wants the files gone when
    /// it ends names its own directory with [`state`](Self::state).
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
        // Dropped rather than reported: a path with nothing at it is the state
        // this wants, and that is what a failure to remove one usually means.
        // What it must not be is left, because the run below would read it.
        drop(std::fs::remove_dir_all(&root));
        Self::new()
            .opening(<G::State as Opens>::opening())
            .rate(G::PERIOD)
            .headless()
            .state(root)
            .settings(Settings::default())
    }
}
