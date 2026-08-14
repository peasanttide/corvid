//! The builder calls that decide what session is played and by whom.
//!
//! The seam against `settings.rs` is who the setting is about: those are about
//! this *machine* -- where its files go, what clock it runs on, whether it
//! opens a window -- and these are about the *session* every peer shares, plus
//! the seat this machine takes in it.

//! The builder calls: everything a run can be told before it starts.
//!
//! The seam against `opening.rs` is that nothing here can fail. Each of these
//! writes one field and hands the app back, which is what lets them chain; the
//! calls that read a file or a command line are next door.

use corvid_behavior::PlayerId;
use corvid_input::Input;
use corvid_replay::Opening;
use corvid_signal::Emitter;
use corvid_time::{Tick, Ticks};

use crate::app::{App, Progress, Stop};
use crate::cli::Arguments;
use crate::game::{AuralizerConfig, BotConfig, ControllerConfig, Game, RenderConfig};
use crate::seating::Seating;

impl<G: Game> App<G>
where
    ControllerConfig<G>: Default,
    BotConfig<G>: Default,
    RenderConfig<G>: Default,
    AuralizerConfig<G>: Default,
{
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
    /// [`Error::Seat`](crate::Error::Seat) whichever of the three it is.
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
    /// [`Error::BotsAndPeers`](crate::Error::BotsAndPeers). The bot is asked only where a run plays alone --
    /// a linked run submits this client's action and never calls it -- so a run
    /// that took both would have accepted a number of bots and played none of
    /// those seats.
    #[must_use]
    pub const fn bots(mut self, count: u16) -> Self {
        self.bots = count;
        self
    }

    /// Watch a seat without playing it.
    ///
    /// The camera, the renderer and the ears are the watched seat's, and
    /// nothing is submitted for it: the column is filled by a peer or a bot, or
    /// holds the idle action. The controller is not asked for one either --
    /// [`action`](corvid_control::Controller::action) is not called at all on a
    /// run that plays nobody -- so a spectator costs the run the whole of what
    /// deciding an action costs rather than only the write.
    ///
    /// The seat watched is whichever [`seat`](Self::seat) named, and the
    /// roster's first for a run that named none -- so `--spectator --seat 1`
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
    /// arrived is folded in -- rolling back when a real action disagrees with
    /// what this machine predicted -- and one datagram goes out per tick
    /// carrying this seat's newest actions and the digest of its state.
    /// `State` and `Present` are the same two implementations they were.
    ///
    /// Which seat this machine is is [`seat`](Self::seat), and
    /// [`seat_of`](crate::seat_of) is the map between a seat and the machine
    /// playing it: two processes started by one command line have that, and a
    /// session assembled by a lobby is told otherwise over
    /// [`Channel::Control`](corvid_net::Channel).
    ///
    /// # What changes about a run
    ///
    /// The tick rate does not, the digest of a given action log does not, and
    /// the frames a client draws do not. What does is that the run's tick is
    /// the peer's: it may stall -- [`Budget::ahead`](corvid_lockstep::Budget)
    /// past the tick every seat has confirmed, a peer waits rather than
    /// predicts further -- and it may go backwards when a correction arrives,
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
    /// machine and the link rather than of the session -- two peers with
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
    /// the whole run. **Nothing refills it.** There is no device layer here --
    /// nothing binds, notices a controller arriving, or rebinds -- so a run
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
    /// which is hashed, serialized and sent -- a column existing for a test's
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
    /// A [`Ticks`] rather than a `u64`, because a count and a point in time are
    /// different things and this one is a count: the deadline it becomes is
    /// [`Ticks::after`] the tick the run opened on, which is the arithmetic the
    /// two types exist to keep apart.
    ///
    /// `for_ticks(Ticks::NONE)` stops before the first tick, which is a run of
    /// no ticks rather than a run without end -- the predicate is checked after
    /// each tick, so the zero case is answered by the loop's own bound rather
    /// than by the predicate, and [`Outcome::state`](crate::Outcome::state) is the opening state.
    #[must_use]
    pub const fn for_ticks(mut self, ticks: Ticks) -> Self {
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
    /// them, after every other builder call has had its say -- an ordinary
    /// setter would be overwritten by a `for_ticks` two lines further down, and
    /// a game's `main` would silently ignore `--ticks`. Saying it twice keeps
    /// the second, because two command lines is one command line and the later
    /// one is the one being asked for.
    ///
    /// [`launch`](Self::launch) is this and [`run`](Self::run) together, and is
    /// what a game's `main` normally calls. This is the seam for a game that
    /// wants to read the arguments itself -- to answer `--help` on its own
    /// stdout, or to accept flags of its own alongside these.
    #[must_use]
    pub fn arguments(mut self, arguments: Arguments) -> Self {
        self.arguments = Some(arguments);
        self
    }
}
