//! The openings these tests play, and the four games that play them.
//!
//! The seam is assembly: every type the games are made of is declared in the
//! three files beside this one, and this is where they are named together.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use corvid_behavior::{Command, PlayerId, PlayerState, Presence, ProfileId};
use corvid_control::{Acting, Controller, Updating};
use corvid_hash::{Digest, digest};
use corvid_input::{Digital, Input};
use corvid_replay::{Opening, Opens, Profile, Schema, Seed};
use corvid_time::{Tick, TickSpan};
use serde::{Deserialize, Serialize};

use super::client::{Ears, Hands};
use super::painted::Painted;
use super::{Action, FIELD, Level, Rules, Tally, action};

/// One player, as the tick was handed them.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Seen {
    /// The seat the runtime attributed this action to.
    pub(crate) id: PlayerId,
    /// Where the runtime says this player stands this tick.
    pub(crate) presence: Presence,
    /// Whether the action in this column came from this client's controller
    /// rather than being the default every other seat submits.
    pub(crate) mine: bool,
    /// The bits of the alpha the controller was handed, for the column that
    /// has one.
    pub(crate) alpha: u16,
}

/// What one tick was handed, in the order it was handed it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Roll {
    /// One entry per player the tick saw.
    pub(crate) seats: Vec<Seen>,
}

/// Every tick's roster, and nothing else.
///
/// **There is no tick counter here**, and its absence is the point. A run of a
/// fixed length is [`App::for_ticks`](corvid_app::App::for_ticks), and a
/// predicate that wants the tick is handed it. A counter kept for a test's
/// benefit would be hashed, serialized and sent every tick, so there is none.
/// The index into [`rolls`](Self::rolls) is the tick's offset
/// from the opening, which is a fact about the vector rather than a column.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Attendance {
    /// One entry per tick that has run, in order.
    pub(crate) rolls: Vec<Roll>,
}

/// One player's intent: a record of what `action` was handed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Mark {
    /// True only for an action `action` built, so a seat holding
    /// [`Default`] is distinguishable from one this client submitted for.
    pub(crate) mine: bool,
    /// The bits of the frame's alpha at the moment `action` ran.
    pub(crate) alpha: u16,
}

impl corvid_behavior::State for Attendance {
    const NAME: &'static str = "census";

    type Level = Level;
    type Rules = Rules;
    type Action = Mark;

    fn tick(
        self,
        _level: &Level,
        players: &[PlayerState<Mark>],
        _rules: &Rules,
        _command: &mut impl Command,
    ) -> Self {
        let mut rolls = self.rolls;
        rolls.push(Roll {
            seats: players
                .iter()
                .map(|player| Seen {
                    id: player.id,
                    presence: player.presence,
                    mine: player.action.mine,
                    alpha: player.action.alpha,
                })
                .collect(),
        });
        Self { rolls }
    }
}

/// A profile that joined on the opening tick and has not left.
pub(crate) const fn seat(account: u64) -> Profile {
    Profile {
        account: ProfileId(account),
        joined: Tick::ZERO,
        left: None,
    }
}

/// An opening for [`Attendance`] with the roster given.
///
/// The roster is the argument because that is what the tests using this game
/// vary: how many seats there are, when each of them joined, and which of them
/// this client submits for.
pub(crate) fn attendance(roster: Vec<Profile>) -> Opening<Attendance> {
    Opening {
        level: FIELD.to_owned(),
        content: Arc::new(Level {
            name: FIELD.to_owned(),
        }),
        rules: Arc::new(Rules::quiet()),
        roster,
        seed: Seed(0x5eed),
        first: Tick::ZERO,
        origin: None,
        schema: Schema::new("census")
            .field("Attendance.rolls", "Vec<Roll>")
            .field("Roll.seats", "Vec<Seen>")
            .field("Seen", "PlayerId | Presence | bool | u16")
            .digest(),
    }
}

/// The description of these types, which a capture records and a load compares.
pub(crate) fn schema() -> Digest {
    Schema::new("tally")
        .field("State.count", "i64")
        .field("State.now", "Tick")
        .field("State.movers", "Vec<PlayerId>")
        .field("Action", "Idle | Bump")
        .digest()
}

/// An opening for either game: one seat, joining on the first tick, with the
/// rules given.
pub(crate) fn opening<S>(rules: Rules) -> Opening<S>
where
    S: corvid_behavior::State<Level = Level, Rules = Rules> + Default,
{
    Opening {
        level: FIELD.to_owned(),
        content: Arc::new(Level {
            name: FIELD.to_owned(),
        }),
        rules: Arc::new(rules),
        roster: vec![Profile {
            account: ProfileId(1000),
            joined: Tick::ZERO,
            left: None,
        }],
        seed: Seed(0x5eed),
        first: Tick::ZERO,
        // `None`, which is `S::default()` -- and both fixture states open on
        // theirs, so nothing is lost by not stating it.
        origin: None,
        schema: schema(),
    }
}

/// An input snapshot with the rest button held, which is the one thing a test
/// can say to `action` when nothing refills the snapshot.
pub(crate) fn resting() -> Input {
    let mut input = Input::new(action::SETS);
    input.set_digital(action::REST, Digital::HELD);
    input
}

/// The digest of a state, spelled out so a test reads as an assertion about
/// states rather than about hashing.
pub(crate) fn mark(state: &Tally) -> Digest {
    digest(state)
}

/// A directory under the system's temporary one, removed when this is dropped.
///
/// Written here rather than taken from a crate because it is twenty lines and
/// because a capture test wants to know exactly which paths exist -- a helper
/// that hid the path would be hiding the thing under test.
#[derive(Debug)]
pub(crate) struct Scratchpad {
    /// Where it is.
    path: PathBuf,
}

impl Scratchpad {
    /// A directory nothing else is using, named for the test that asked.
    ///
    /// It is not created here. `App::capture` is what creates a capture
    /// directory, and a test that found one already there would not be testing
    /// that.
    pub(crate) fn new(what: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("corvid_app-{}-{what}-{unique}", std::process::id()));
        drop(fs::remove_dir_all(&path));
        Self { path }
    }

    /// Where it is.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratchpad {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.path));
    }
}

/// Where a run of [`Tally`] that was told nothing else starts.
///
/// Every test here says [`App::opening`](corvid_app::App::opening) for itself,
/// because what a test varies is the rules: which tick quits, which tick saves,
/// which tick asks for a screenshot. This is here because a
/// [`Game`](corvid_app::Game) names a state that can open a session on its own,
/// and the quiet rules are the honest answer for a run nobody has configured.
impl Opens for Tally {
    fn opening() -> Opening<Self> {
        opening(Rules::quiet())
    }
}

/// The same, for the roster fixture: one seat, joined on the first tick.
impl Opens for Attendance {
    fn opening() -> Opening<Self> {
        attendance(vec![seat(1000)])
    }
}

corvid_app::game! {
    /// The game the tests in this crate play: the tally simulates, [`Hands`]
    /// plays it, nothing bots for it, [`Painted`] draws it and [`Ears`] hears
    /// it.
    ///
    /// [`CRADLE`](corvid_time::TickSpan::CRADLE) is the period because it is
    /// the rate every timed assertion in these tests was written against -- a
    /// marker that chose another one would change how many ticks a run of a
    /// fixed duration simulates, which is a different run and a different
    /// digest.
    pub(crate) struct Counting;
    const PERIOD: TickSpan = TickSpan::CRADLE;
    type State = Tally;
    type Controller = Hands;
    type Render = Painted;
    type Auralizer = Ears;
}

corvid_app::game! {
    /// The tally with nobody playing it: a dedicated server, as a game.
    ///
    /// Four types unnamed, which is what a game that reads no device, runs no
    /// bot, opens no adapter and opens no sound card writes. It is here for the
    /// settings tests, whose subject is the *document* rather than what is in
    /// it -- four configs of `()` are four fields that still have to be named,
    /// written down and read back.
    pub(crate) struct Bare;
    const PERIOD: TickSpan = TickSpan::CRADLE;
    type State = Tally;
}

corvid_app::game! {
    /// The tally with a bot in it, which is the game the bot tests play.
    ///
    /// Its controller is `()` and its [`Bot`](corvid_app::Game::Bot) is
    /// [`Nudge`], which is what makes a run of it readable as a column at a
    /// time: every non-idle action in the log came from a bot, because the only
    /// other thing writing one answers [`Action::Idle`] and a row nobody wrote
    /// holds the same.
    ///
    /// Nothing is drawn and nothing is heard, because what these tests are
    /// about is which seats got filled.
    pub(crate) struct Botted;
    const PERIOD: TickSpan = TickSpan::CRADLE;
    type State = Tally;
    type Bot = Nudge;
}

/// The bot for [`Tally`]: a bump, every tick, for whatever seat it is asked
/// about.
///
/// Being *distinguishable* is the whole requirement. [`Action::Idle`] is what a
/// row nobody wrote holds and what the `()` controller answers, so an
/// unconditional [`Action::Bump`] is the one answer that separates "a bot
/// played this seat" from "nothing did".
///
/// It ignores [`Acting::seat`], which is the honest thing for a bot with one
/// opinion to do: what the seat is for is telling several apart, and this one
/// plays them all the same.
///
/// `REAL` is false, which is what a controller with nobody behind it says. It
/// is about the platform rather than about whether the controller runs, and
/// this one is asked for an action every tick of every seat it plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Nudge;

impl Controller<Tally> for Nudge {
    type Config = ();

    const REAL: bool = false;
    const SETS: &'static [corvid_input::SetDescriptor] = &[];

    fn new((): ()) -> Self {
        Self
    }

    fn configure(&mut self, (): ()) {}

    fn action(&self, _acting: Acting<'_, Tally>) -> Action {
        Action::Bump
    }

    fn update(&mut self, _updating: Updating<'_, Tally>) {}

    fn look(&self) -> corvid_camera::Camera {
        corvid_camera::Camera::default()
    }
}

corvid_app::game! {
    /// The game the roster tests play: [`Attendance`], with nothing drawn and
    /// nothing heard.
    ///
    /// A marker of its own beside [`Counting`] rather than a parameter on it.
    /// What these tests are about is the *arguments* a tick was handed -- which
    /// seats were in the roster, and which column this client's action landed
    /// in -- and a run of them opens no device and makes no sound, so the two
    /// types that would have to be `()` for one game and real for the other are
    /// left unnamed here.
    pub(crate) struct Attending;
    const PERIOD: TickSpan = TickSpan::CRADLE;
    type State = Attendance;
    type Controller = Marker;
}

/// The controller for [`Attendance`]: it marks every action as its own.
///
/// The alpha column is written as zero and stays zero: a controller's `action`
/// runs once per tick and never sees an interpolation weight, because
/// interpolation is the renderer's and happens in a shader. The test that reads
/// the column says the same thing from the other side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Marker;

impl Controller<Attendance> for Marker {
    type Config = ();

    const SETS: &'static [corvid_input::SetDescriptor] = &[];

    fn new((): ()) -> Self {
        Self
    }

    fn configure(&mut self, (): ()) {}

    fn action(&self, _acting: Acting<'_, Attendance>) -> Mark {
        Mark {
            mine: true,
            alpha: 0,
        }
    }

    fn update(&mut self, _updating: Updating<'_, Attendance>) {}

    fn look(&self) -> corvid_camera::Camera {
        corvid_camera::Camera::default()
    }
}
