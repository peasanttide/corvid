//! Playing: the three backends, and the plan each of them is handed.
//!
//! The seam against the builder is that this is where the settings stop being
//! settings. Nothing here writes a field; it reads what was set, decides which
//! backend the run wants, and hands the thread over to it.

use std::path::Path;

use corvid_behavior::{PlayerId, State};
use corvid_time::{Clock, Elapsed};

use crate::app::{App, Error, Outcome};
use crate::capture::Capture;
use crate::cli::Arguments;
use crate::game::{AuralizerConfig, BotConfig, ControllerConfig, Game, RenderConfig};
use crate::retention::Retention;
use crate::runtime::Plan;
use crate::saves::Saves;
use crate::settings::Settings;

impl<G: Game> App<G>
where
    ControllerConfig<G>: Default,
    BotConfig<G>: Default,
    RenderConfig<G>: Default,
    AuralizerConfig<G>: Default,
{
    /// Reads the standard arguments and plays the game.
    ///
    /// [`main`](crate::main) is what a game writes and this is the same reading
    /// of the command line for a harness that has already built an [`App`](crate::App) of
    /// its own -- one with a clock it chose, a seat it chose, or a stop
    /// predicate no flag can express -- and wants the operator's word applied on
    /// top of it.
    ///
    /// ```no_run
    /// # use core::convert::Infallible;
    /// # use corvid_replay::Opens;
    /// # use serde::{Deserialize, Serialize};
    /// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    /// # struct Nowhere;
    /// # impl corvid_behavior::Level for Nowhere {
    /// #     type Error = Infallible;
    /// #     fn load(_: &str) -> Result<Self, Infallible> { Ok(Self) }
    /// # }
    /// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    /// # struct Bounce;
    /// # impl corvid_behavior::State for Bounce { /* ... */
    /// #     const NAME: &'static str = "bounce";
    /// #     type Level = Nowhere; type Rules = (); type Action = ();
    /// # }
    /// # impl corvid_replay::Opens for Bounce {
    /// #     fn opening() -> corvid_replay::Opening<Self> { unimplemented!() }
    /// # }
    ///     ///
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
    /// [`Error::Argument`](crate::Error::Argument) for anything the command line could not be read as --
    /// including `--help`, which is not a failure and arrives as one because
    /// this crate may not print -- and then whatever [`run`](Self::run) reports.
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
    /// [`Error::Argument`](crate::Error::Argument) for a `--level` in the
    /// [`arguments`](Self::arguments) that this game cannot open on -- the one
    /// flag whose value only the game can judge, and so the one that is refused
    /// here rather than by the parser. Then [`Error::Unopened`](crate::Error::Unopened) if no opening
    /// was given, [`Error::Shape`](crate::Error::Shape) if the opening cannot be made into a
    /// session, [`Error::NoSeats`](crate::Error::NoSeats) if that session's roster is empty,
    /// [`Error::Seat`](crate::Error::Seat) if the seat is not in the roster of the session the run
    /// ends up playing, [`Error::BotsAndPeers`](crate::Error::BotsAndPeers) if it asked for bots and a
    /// transport at once, [`Error::Log`](crate::Error::Log) if the action log refuses a write, and
    /// [`Error::Wrote`](crate::Error::Wrote) or [`Error::Encoded`](crate::Error::Encoded) if a capture or a recording
    /// cannot be written. A run with a device adds [`Error::Drew`](crate::Error::Drew), and a
    /// windowed one adds [`Error::NoWindow`](crate::Error::NoWindow).
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
        // -- a run that is not going to happen should not have created a capture
        // directory on the way to saying so.
        #[cfg(feature = "net")]
        if networked && self.bots > 0 {
            return Err(Error::BotsAndPeers { bots: self.bots });
        }
        // The one directory this game keeps anything in, resolved once and
        // handed to everything that writes: the slots, the settings file and --
        // on the windowed path -- the binding file. Three lookups would be three
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
    /// Read while the transport is still on the builder -- [`prepare`](Self::prepare)
    /// moves it into the [`Plan`] -- because it is what
    /// [`clock`](Self::clock) defaults on.
    #[cfg_attr(
        not(feature = "net"),
        expect(
            clippy::unused_self,
            reason = "the transport this reads is what `net` adds, and a build without one answers about the same app for the same reason -- a free function would put the feature in the call site instead"
        )
    )]
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
    /// [`Error::Unopened`](crate::Error::Unopened), [`Error::Shape`](crate::Error::Shape), [`Error::NoSeats`](crate::Error::NoSeats),
    /// [`Error::Seat`](crate::Error::Seat), and whatever opening a capture directory or reading a
    /// save reported.
    fn prepare(&mut self, root: &Path) -> Result<(Plan<G::State>, Option<Capture>), Error> {
        let opening = self.opening.take().ok_or(Error::Unopened)?;
        let saves = Saves::under(root);

        let (session, resumed) = self.open(opening, &saves)?;

        // Against the roster that is actually in force, which on a `--load` or
        // a `--demo` is the resumed session's and not the fresh opening's --
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
        let deadline = self.ticks.map(|ticks| ticks.after(opened));

        // The one default that reads another setting. A run nobody is recording
        // keeps a window, and a run being written down keeps the lot -- because
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
    /// it -- whichever [`clock`](Self::clock) was given, or the default below.
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
            // endpoint states -- one tick per reading, as fast as the processor
            // allows -- and a peer pacing itself that way spends every tick it
            // is ahead of the session spinning against a frontier that only
            // moves when a *real* second has passed on somebody else's machine.
            // It converges either way; it converges having burned a core.
            if networked {
                return Box::new(corvid_time::Clock::wall()) as Box<dyn Elapsed>;
            }
            Box::new(Clock::stepping(self.rate.period()))
        })
    }
}
