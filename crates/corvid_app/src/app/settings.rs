//! The builder calls: everything a run can be told before it starts.
//!
//! The seam against `opening.rs` is that nothing here can fail. Each of these
//! writes one field and hands the app back, which is what lets them chain; the
//! calls that read a file or a command line are next door.

use std::path::PathBuf;

use corvid_behavior::SaveSlot;
use corvid_time::{Elapsed, TickSpan};

use crate::app::App;
use crate::game::{AuralizerConfig, BotConfig, ControllerConfig, Game, RenderConfig};
use crate::retention::Retention;
use crate::settings::Settings;

impl<G: Game> App<G>
where
    ControllerConfig<G>: Default,
    BotConfig<G>: Default,
    RenderConfig<G>: Default,
    AuralizerConfig<G>: Default,
{
    /// Overrides what the player has set, for this run only.
    ///
    /// **Nothing needs this.** A run reads
    /// `$XDG_CONFIG_HOME/<NAME>/setting.json` and starts from what is in it, or
    /// from the defaults where there is no file -- which is what a fresh install
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
    /// `const` because undoing the two settings is two assignments: they are a
    /// `bool` and an [`Option<Extent>`](corvid_render::Extent), and there is no
    /// resource to drop alongside them. `mut self` earns its keep only in a
    /// build that has one of the two to undo, which is what the attribute says.
    #[must_use]
    #[cfg_attr(
        not(any(feature = "window", feature = "render")),
        expect(
            unused_mut,
            reason = "with neither device feature there is nothing to unset, and a build of this crate that could not be asked for a headless run would be worse than one where the call is a no-op"
        )
    )]
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
    /// opens at all would be a person looking at one --
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
    /// Under it are `saves/`, the settings file and -- for a windowed run -- the
    /// binding file. **One directory rather than three**: a player who copies a
    /// game to another machine copies one path, and a test that must not touch
    /// theirs redirects one call.
    ///
    /// The default is `$XDG_DATA_HOME/NAME/`, from the game's
    /// [`NAME`](corvid_behavior::State::NAME) -- so `~/.local/share/NAME/` on a machine that has
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
    /// the same bytes a [`capture`](Self::capture)'s `session` file holds -- so
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
    /// written is [`Error::Empty`](crate::Error::Empty), because a run that was asked to resume and
    /// silently started a new game would be a run that lost somebody's save.
    ///
    /// Reading is [`Session::seek`](corvid_replay::Session::seek), which is the
    /// same call rollback and time-walk are -- so a save that cannot be replayed
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
    /// and a captured run can be told to keep a window -- which records a capture
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
    /// [`rate`](Self::rate)'s **own** period -- a reading is one period, so a
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
    /// did not choose -- a soak test compressing an hour, a benchmark timing one
    /// tick -- and a run like that has nobody on the other end of a link to
    /// disagree with.
    #[must_use]
    pub const fn rate(mut self, rate: TickSpan) -> Self {
        self.rate = rate;
        self
    }
}
