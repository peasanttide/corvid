//! Two games and a temporary directory.
//!
//! [`Tally`] is a complete `State` **and** `Present` implementation that
//! keeps every promise the contracts make, and every part of it is there to be
//! falsifiable by one of the tests:
//!
//! * its action varies with the tick, so a loop that logged an action against
//!   the wrong tick produces a different session rather than the same one;
//! * its action also varies with the wall time `look` has been handed, which is
//!   the only way a clock that is not the app's clock can reach the log;
//! * its state owns a `Vec` built fresh every tick, so a state is a real
//!   allocation rather than a handful of integers and a digest of one covers a
//!   column whose length varies with what the players did;
//! * its rules name the ticks it asks the platform for things on, so a test
//!   picks which requests a run makes without going through an input layer,
//!   which a headless run does not have.
//!
//! [`Leaky`] is the same game with one line changed: its tick reads a counter
//! it has been accumulating in its `Scratch`, which `corvid_behavior` forbids.
//! It is here because the `dev` schedule exists to find exactly that, and a
//! check for a leak needs something that leaks.
//!
//! [`backstop`] is the other half of this module and has nothing to do with
//! games: it is how a test that watches a run from a second thread fails rather
//! than hangs.
//!
//! [`Nudge`] is the bot, and it is one line of opinion: it answers
//! [`Action::Bump`] for every seat it is given, every tick. A run of [`Botted`]
//! therefore reads a column at a time -- a seat holding a bump is a seat the
//! runtime filled, and a seat holding the idle action is one nothing did.
//!
//! [`Attendance`] is the third, and it exists because the two above cannot see the
//! loop's *arguments*. Its tick writes down the roster it was handed -- every
//! seat, in order, with its presence and with whether its action was this
//! client's -- so a run of it is a record of who the loop said was playing and
//! which column this client's action landed in. Its own state holds no tick
//! counter, which is the second thing it is here to demonstrate: with
//! [`App::for_ticks`](corvid_app::App::for_ticks) a run of a fixed length
//! costs a game nothing on the wire.

#![allow(
    dead_code,
    reason = "each integration test binary compiles this module separately, so anything only one of them uses is dead in the others"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "this module is private to each test binary, so pub(crate) and pub are equivalent -- pub(crate) is the one rustc's unreachable_pub asks for, and the two lints cannot both be satisfied"
)]

pub(crate) mod backstop;
mod client;
mod games;
mod painted;
pub(crate) mod traced;

// Named here so a test says `common::Counting` rather than
// `common::games::Counting`: the split below is about file size and is not a
// thing a test has any reason to know about. `unused_imports` because each
// test binary compiles this module separately and uses a different part of it,
// which is the same reason `dead_code` is allowed above.
#[expect(
    unused_imports,
    reason = "each integration test binary compiles this module separately and names a different part of it"
)]
pub(crate) use client::{Ears, Hands, Holding};
#[expect(
    unused_imports,
    reason = "as above: one test binary's unused re-export is another's whole fixture"
)]
pub(crate) use games::{
    Attendance, Attending, Bare, Botted, Counting, Mark, Marker, Nudge, Roll, Scratchpad, Seen,
    attendance, mark, opening, resting, schema, seat,
};
#[expect(
    unused_imports,
    reason = "as above: only the tests that open a device name the drawing half"
)]
pub(crate) use painted::Painted;

use corvid_behavior::{AchievementId, Command, ExitCode, PlayerId, PlayerState, SaveSlot};
use corvid_sound::{SoundId, SourceId};
use corvid_time::{Duration, Tick};
use serde::{Deserialize, Serialize};

/// The one place this game's actions are named.
pub(crate) mod action {
    corvid_input::action_sets! {
        pub set Playing {
            digital REST;
        }
    }
}

/// What this fixture's loader refuses [`ELSEWHERE`] with.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0} is kept in a file, and there is nothing to read it from")]
pub(crate) struct Unreadable(pub(crate) String);

/// The level every session here opens on.
pub(crate) const FIELD: &str = "field";

/// A level this fixture keeps in a file, and so cannot build from its name.
///
/// [`Level::load`](corvid_behavior::Level::load) refuses it, which is what a
/// game whose levels are read from disk does when it is handed a source with
/// nothing in it -- the source a `--level` on the command line has to offer.
pub(crate) const ELSEWHERE: &str = "elsewhere";

/// The slot the game saves into.
pub(crate) const SLOT: SaveSlot = SaveSlot(2);

/// The status the game quits with, chosen so that a run which stopped because
/// [`until`](corvid_app::App::until) said so cannot be mistaken for one that
/// quit.
pub(crate) const FAREWELL: ExitCode = ExitCode(7);

/// The achievement the game asks for, which is a request the runtime does not
/// handle.
pub(crate) const APPLAUSE: AchievementId = AchievementId(1);

/// The voice the tally hums through, and the first of the ones the pips use.
pub(crate) const VOICE: SourceId = SourceId(1);

/// What it hums.
pub(crate) const HUM: SoundId = SoundId(1);

/// What a bump rings.
pub(crate) const CHIME: SoundId = SoundId(2);

/// Authored, immutable within a session.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Level {
    /// The name the runtime would have loaded this by.
    pub(crate) name: String,
}

/// Deterministic tuning every peer agrees on, and the four ticks this game
/// asks the platform for something on.
///
/// The requests are keyed to ticks rather than to actions because a headless
/// run has no device layer: an action comes from `action`, and `action`
/// is handed an input snapshot nothing refills. A tick that knows its own
/// number can ask for a save on tick seven without anybody pressing anything.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Rules {
    /// How far one bump moves the tally.
    pub(crate) step: i64,
    /// The tick that asks to quit.
    pub(crate) quit_at: Option<Tick>,
    /// What that tick asks to quit *with*, or [`FAREWELL`] when it says
    /// nothing. It is a setting rather than a constant so that a test can put
    /// either of two statuses first and read the difference.
    pub(crate) quit_with: Option<ExitCode>,
    /// A second status the same tick asks to quit with, right after the first.
    ///
    /// Two `Quit`s out of one tick is a legal thing for a game to return -- the
    /// vocabulary says nothing against it, and a game that quits from two
    /// places in one tick writes exactly this -- and the sink documents that the
    /// first one wins. Without a game that emits both, that sentence is a
    /// comment on a branch nothing takes.
    pub(crate) then_quit_with: Option<ExitCode>,
    /// The tick that asks for a save.
    pub(crate) save_at: Option<Tick>,
    /// The tick that asks for it back.
    pub(crate) read_at: Option<Tick>,
    /// The tick that asks for an achievement, which nothing here handles.
    pub(crate) cheer_at: Option<Tick>,
    /// The tick that asks for a screenshot.
    pub(crate) snap_at: Option<Tick>,
    /// The tick the client-local half stops the clock on.
    ///
    /// A pause is not a request and does not go through `Command`: it is
    /// `Controller::simulating`, which the runtime asks the *view* about. So this
    /// is read by `look` rather than by the tick, and what it moves is a field
    /// of the view. It is a rule only because a headless run has no device
    /// layer to press a key on, which is the reason every other setting here is
    /// a rule too.
    pub(crate) pause_at: Option<Tick>,
    /// How many displayed frames the pause lasts.
    pub(crate) pause_for: u64,
}

impl Rules {
    /// Rules that ask for nothing.
    pub(crate) const fn quiet() -> Self {
        Self {
            step: 3,
            quit_at: None,
            quit_with: None,
            then_quit_with: None,
            save_at: None,
            read_at: None,
            cheer_at: None,
            snap_at: None,
            pause_at: None,
            pause_for: 0,
        }
    }
}

/// Everything that cannot be recomputed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Tally {
    /// The tally.
    pub(crate) count: i64,
    /// Which tick this state is at. A tick is not handed its own number, so a
    /// state that wants one counts.
    pub(crate) now: Tick,
    /// Who bumped on the tick that produced this state.
    ///
    /// The column exists so that the state owns an allocation rather than being
    /// two integers a machine copies without noticing -- a run of this game is a
    /// run in which every tick allocates, which is what the retention and
    /// capture tests are measuring the cost of.
    pub(crate) movers: Vec<PlayerId>,
}

/// One player's intent for one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum Action {
    /// Did nothing, and what every seat but this client's submits.
    #[default]
    Idle,
    /// Move the tally by one step.
    Bump,
}

/// What [`Tally`] carries from tick to tick, and never reads.
///
/// The degenerate memo: written every tick, consulted by nothing, so whatever
/// the runtime does to it -- carry it, or throw it away on the `dev` schedule --
/// the states are the same states. That is exactly what makes it useful here.
/// `tests/dev.rs` discards this on a schedule and checks that [`Tally`] agrees
/// with itself anyway, which is only a check at all if there is a real value
/// being discarded; a `Scratch` of `()` would make the honest arm of that test
/// pass by construction.
///
/// It is a counter rather than a pool because there is no longer anywhere for a
/// pool to get its buffers back from: a retiring state is a handle the runtime
/// may not hold the last one of, so [`Tally::tick`] builds its column with
/// [`Vec::new`] and lets it go with the state.
#[derive(Debug, Default)]
pub(crate) struct Odometer {
    /// How many ticks this scratch has been carried through.
    ticks: u64,
}

/// Camera and cosmetics: never hashed, never sent, never rolled back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct View {
    /// How much wall-clock time `look` has been handed.
    ///
    /// [`Tally::action`] reads it, which `corvid_control` warns is a display
    /// -rate quantity reaching an action. It is deliberate here: it is the one
    /// route by which a clock the app was not given could reach the session,
    /// and `tests/headless.rs` is what walks it.
    pub(crate) elapsed: Duration,
    /// How many times `look` has been called.
    pub(crate) frames: u64,
    /// How many displayed frames have happened since the pause began.
    pub(crate) held: u64,
    /// Whether the simulation is stopped, which is what
    /// [`Controller::simulating`] answers with.
    pub(crate) paused: bool,
}

/// The commands this game's rules ask for at `now`.
fn requests(rules: &Rules, now: Tick, command: &mut impl Command) {
    if rules.save_at == Some(now) {
        command.save(SLOT);
    }
    if rules.read_at == Some(now) {
        command.read(SLOT);
    }
    if rules.cheer_at == Some(now) {
        command.achieve(APPLAUSE);
    }
    if rules.snap_at == Some(now) {
        command.screenshot();
    }
    // Last, so that a tick which both asks for something and asks to stop has
    // its other request drained before the loop breaks. The sink takes the
    // whole list either way; the order is what a reader of `Requests` sees.
    if rules.quit_at == Some(now) {
        command.quit(rules.quit_with.unwrap_or(FAREWELL));
        if let Some(second) = rules.then_quit_with {
            command.quit(second);
        }
    }
}

/// Every level but one is its own name: this fixture builds what it is asked
/// for and reads nothing.
///
/// [`ELSEWHERE`] is the exception, and it is here so that both answers a
/// `--level` can get are reachable. A game whose levels are self-describing
/// opens on the one named; a game that reads a level out of files refuses when
/// the source it is handed has none, and this loader stands in for the second
/// without the fixture needing a file.
impl corvid_behavior::Level for Level {
    type Error = Unreadable;

    fn load(name: &str) -> Result<Self, Unreadable> {
        if name == ELSEWHERE {
            return Err(Unreadable(name.to_owned()));
        }
        Ok(Self {
            name: name.to_owned(),
        })
    }
}

impl corvid_behavior::State for Tally {
    const NAME: &'static str = "tally";

    type Level = Level;
    type Rules = Rules;
    type Action = Action;

    fn tick(
        self,
        _level: &Level,
        players: &[PlayerState<Action>],
        rules: &Rules,
        command: &mut impl Command,
    ) -> Self {
        // Fresh every tick. The state owns this column and hands it to whoever
        // holds the state, and nothing gives it back.
        let mut movers = Vec::new();
        let mut count = self.count;
        for player in players {
            if matches!(player.action, Action::Bump) {
                count += rules.step;
                movers.push(player.id);
            }
        }
        requests(rules, self.now, command);
        Self {
            count,
            now: self.now.next(),
            movers,
        }
    }
}
