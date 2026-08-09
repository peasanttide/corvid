//! One game that can be told to misbehave in one specific way, and the
//! openings that tell it to.
//!
//! Every check in this crate is a falsifier, so every test of one needs a game
//! that actually falsifies it — and a test that only ever points a check at a
//! correct game is a test that would pass against a check that returns `Ok`
//! unconditionally.
//!
//! # Why one game rather than eight
//!
//! Eight fixtures would be eight `Simulate` impls, eight `Present` impls, eight
//! `Render` implementations and eight openings, differing in one line each. What is under test here is the
//! *checks*, so the game is a constant and the misbehaviour is a value:
//! [`Habit`] is part of the [`Rules`](Simulate::Rules), which is where a game
//! puts the tuning every peer has to agree on, and a session's habit is
//! therefore part of what it is rather than a switch beside it.
//!
//! # The spin, and why it is an array
//!
//! Half the habits below need something that reads differently on two runs of
//! one opening, and a `no_std` simulation has no clock and no environment to
//! reach for. What it does have is a process-global with interior mutability —
//! the hole `corvid_behavior` says is discipline rather than structure — so that
//! is what these use, and reaching one is exactly the bug
//! [`is_reproducible`](corvid_test::is_reproducible) exists to find.
//!
//! It is an array of them, indexed by the rules, because the two runs a check
//! makes have to be the first and second consumers of their counter for the
//! difference between them to be a known number. Tests in one binary run in
//! parallel, so a counter shared between two tests would give each of them a
//! starting value that depends on the other. **One spin index per test**, and
//! the tests say which they use.

#![allow(
    dead_code,
    reason = "each integration test binary compiles this module separately, so anything only one of them uses is dead in the others"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "this module is private to each test binary, so pub(crate) and pub are equivalent — pub(crate) is the one rustc's unreachable_pub asks for, and the two lints cannot both be satisfied"
)]

use std::hash::{Hash, Hasher};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use corvid_behavior::{Command, Level, Player, ProfileId, StatId, State};
use corvid_files::{Malformed, Source};
use corvid_hash::Digest;
use corvid_input::Input;
use corvid_replay::{Opening, Opens, Profile, Schema, Seed};
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

/// The counters the misbehaving habits read.
///
/// Eight because eight is more than the tests need and an index that runs off
/// the end would silently share a counter with index zero.
static SPINS: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];

/// The next value of counter `which`, which no two reads of one counter ever
/// share.
fn spin(which: u8) -> i64 {
    let counter = &SPINS[usize::from(which) % SPINS.len()];
    i64::try_from(counter.fetch_add(1, Ordering::Relaxed)).unwrap_or(i64::MAX)
}

/// The one way this game can be wrong at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum Habit {
    /// Nothing. The state is a function of the previous state, the level, the
    /// rules and the actions, and of nothing else.
    #[default]
    Steady,
    /// The tick folds a process-global into the state.
    Restless,
    /// The tick reports a process-global to the platform and leaves the state
    /// alone.
    Chatty,
    /// `action` reads a process-global to choose between two actions the tick
    /// treats identically.
    Fickle,
    /// The tick asks to quit once a process-global reaches the threshold.
    Halting,
    /// The tick folds a process-global into the one field this game's [`Hash`]
    /// does not absorb.
    Blind,
    /// The tick accumulates in its scratch and reads what it accumulated.
    Hoarder,
    /// The tick accumulates in its scratch and reads what it accumulated on
    /// exactly one tick: the one where the count reaches
    /// [`threshold`](Rules::threshold).
    ///
    /// This is [`Hoarder`](Self::Hoarder)'s leak moved to a known tick, and it
    /// is what makes the tick a walk *names* an assertion rather than a
    /// formality. A walk that advanced the scratch at some multiple of the
    /// session's rate would reach the threshold at the wrong tick, or step over
    /// it and never reach it at all, and either is a passing test against
    /// `Hoarder` — whose count is read on every tick and so is caught at the
    /// first one whatever rate the scratch is advanced at.
    Patient,
    /// The tick folds a process-global into the state on the one tick where the
    /// scratch's count reaches [`threshold`](Rules::threshold), and puts the
    /// field back to zero on every other tick.
    ///
    /// [`Restless`](Self::Restless)'s leak with a lifetime: the mark at that one
    /// tick differs between two runs and every mark before and after it agrees,
    /// so the divergence is visible in the trace and nowhere else — not in the
    /// actions, not in the reach, not in the requests, and not in the final
    /// states, which compare equal. A check that only compared the ticks its two
    /// runs still *held* would therefore miss it entirely once the threshold is
    /// further back than the runs' retention window, which is what
    /// `tests/reproducible.rs` points it at.
    Fleeting,
}

/// The level. Authored, immutable within a session, and — in one field —
/// deliberately unable to survive being written down.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Cliff {
    /// What every tick adds, whatever else it does.
    pub(crate) rise: i64,
    /// What every tick also adds, and what a capture does not record.
    ///
    /// Never written down, and `0` on the way back — so a session whose level
    /// has this set replays into different states than it ran, which is a
    /// desync waiting for a peer to load the capture.
    #[serde(skip)]
    pub(crate) hidden: i64,
}

/// Deterministic tuning: which habit, which counter, and where its threshold
/// is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Rules {
    /// How this game misbehaves.
    pub(crate) habit: Habit,
    /// Which of the [`SPINS`] it reads.
    pub(crate) spin: u8,
    /// The value [`Habit::Halting`] and [`Habit::Fickle`] change their minds at.
    ///
    /// Set to the number of ticks a run plays, so that the first of two runs
    /// stays below it for the whole run and the second starts above it.
    pub(crate) threshold: i64,
}

/// Everything that cannot be recomputed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Climb {
    /// How far up.
    pub(crate) metres: i64,
    /// Which tick this state is at.
    pub(crate) now: Tick,
    /// A field this game's [`Hash`] does not absorb and its `Eq` compares.
    pub(crate) unhashed: i64,
    /// A field a capture does not record and this game's [`Hash`] absorbs.
    #[serde(skip)]
    pub(crate) unwritten: i64,
}

/// Hand-written, and wrong on purpose: [`unhashed`](Climb::unhashed) is not
/// absorbed.
///
/// A derive would absorb all four fields. What this leaves out is the field
/// [`Habit::Blind`] moves, which is how a session that two runs digest alike and
/// compare unequal is expressible at all — the case every other comparison in
/// this crate is structurally unable to see, because every other comparison is
/// between digests.
impl Hash for Climb {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.metres.hash(state);
        self.now.hash(state);
        self.unwritten.hash(state);
    }
}

/// One player's intent for one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum Step {
    /// Hands off.
    #[default]
    Rest,
    /// Climb.
    Up,
    /// Climb, and the tick cannot tell this from [`Up`](Self::Up).
    ///
    /// Two actions with one meaning is what lets [`Habit::Fickle`] put a
    /// difference in the action log that never reaches the state, which is the
    /// only way the action comparison is reachable: an action that moved the
    /// state would be reported as a state divergence instead.
    Leap,
}

/// What the tick carries between ticks. A pool in a real game; a counter here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Hoard {
    /// How many ticks this scratch has been through, which is the one thing a
    /// scratch is not allowed to remember.
    pub(crate) seen: i64,
}

/// The cliff reads nothing: this fixture's is a constant.
impl Level for Cliff {
    type Reference = String;

    fn load(_reference: &String, _files: &dyn Source) -> Result<Self, Malformed> {
        Ok(Self::default())
    }
}

impl State for Climb {
    const NAME: &'static str = "wobble";

    type Level = Cliff;
    type Rules = Rules;
    type Action = Step;

    fn tick(
        self,
        level: &Cliff,
        players: &[Player<'_, Step>],
        rules: &Rules,
        command: &mut impl Command<Reference = String>,
    ) -> Self {
        let previous = &self;
        // The tick number, which the state already carries rather than a
        // counter kept beside it: the two would increment together, and only
        // one of them is in the digest.
        let seen = i64::try_from(previous.now.0).unwrap_or(i64::MAX) + 1;
        let climbed = i64::try_from(
            players
                .iter()
                .filter(|player| !matches!(player.action, Step::Rest))
                .count(),
        )
        .unwrap_or(0);

        let mut next = Self {
            metres: previous.metres + level.rise + level.hidden + climbed,
            now: previous.now.next(),
            unhashed: previous.unhashed,
            unwritten: previous.unwritten,
        };

        match rules.habit {
            Habit::Steady | Habit::Fickle => {}
            Habit::Restless => next.metres += spin(rules.spin),
            Habit::Chatty => command.stat(StatId(1), spin(rules.spin)),
            Habit::Halting => {
                if spin(rules.spin) >= rules.threshold {
                    command.quit(corvid_behavior::ExitCode::SUCCESS);
                }
            }
            Habit::Blind => next.unhashed = spin(rules.spin),
            // `seen` is the tick number out of the state. Accumulating a count
            // beside the state instead would be a count of ticks one machine's
            // snapshot budget happened to run, which is a property of that
            // machine rather than of the session.
            Habit::Hoarder => next.metres += seen,
            // The same leak, on one tick only, so that which tick a check names
            // is a fact about the check rather than about the first tick with
            // any history in it.
            Habit::Patient => {
                if seen == rules.threshold {
                    next.metres += seen;
                }
            }
            // Written on every tick rather than only on the threshold one, so
            // that the difference lasts exactly one mark: a field left alone
            // would carry the leaked value forward and turn a one-tick
            // divergence into a permanent one, which every comparison here can
            // already see.
            Habit::Fleeting => {
                next.unwritten = if seen == rules.threshold {
                    spin(rules.spin).saturating_add(1)
                } else {
                    0
                };
            }
        }

        next
    }
}

/// The climber: what a tick's action is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Legs {
    /// The rules this controller was built with, since only the simulation is
    /// handed them now.
    pub(crate) rules: Rules,
}

impl corvid_control::Controller<Climb> for Legs {
    type Config = Rules;

    /// A fixture with nothing to press.
    const SETS: &'static [corvid_input::SetDescriptor] = &[];

    fn new(rules: Rules) -> Self {
        Self { rules }
    }

    fn configure(&mut self, rules: Rules) {
        self.rules = rules;
    }

    fn action(&self, _acting: corvid_control::Acting<'_, Climb>) -> Step {
        if matches!(self.rules.habit, Habit::Fickle)
            && spin(self.rules.spin) >= self.rules.threshold
        {
            Step::Leap
        } else {
            Step::Up
        }
    }

    fn update(&mut self, _updating: corvid_control::Updating<'_, Climb>) {}

    fn look(&self) -> corvid_camera::Camera {
        corvid_camera::Camera::default()
    }
}

/// The level every opening here is on.
pub(crate) const CLIFF: &str = "cliff";

/// How this build describes its own types.
#[must_use]
pub(crate) fn schema() -> Digest {
    Schema::new("wobble")
        .field("Climb.metres", "i64")
        .field("Climb.now", "Tick")
        .digest()
}

/// An opening with the given habit, counter and threshold, on a level and an
/// origin that both survive being written down.
#[must_use]
pub(crate) const fn rules(habit: Habit, spin: u8, threshold: i64) -> Rules {
    Rules {
        habit,
        spin,
        threshold,
    }
}

/// The opening every run here plays from.
#[must_use]
pub(crate) fn opening(habit: Habit, spin: u8, threshold: i64) -> Opening<Climb> {
    Opening {
        level: CLIFF.to_owned(),
        content: Arc::new(Cliff { rise: 1, hidden: 0 }),
        rules: Arc::new(Rules {
            habit,
            spin,
            threshold,
        }),
        roster: vec![Profile {
            account: ProfileId(1),
            joined: Tick::ZERO,
            left: None,
        }],
        seed: Seed(7),
        first: Tick::ZERO,
        origin: None,
        schema: schema(),
    }
}

/// A [`Habit::Steady`] opening whose level carries a field a capture does not
/// record.
///
/// Its first tick is right and every one after it is wrong, because the level is
/// what came back rather than what went down.
#[must_use]
pub(crate) fn opening_on_a_lossy_level() -> Opening<Climb> {
    let mut opening = opening(Habit::Steady, 0, 0);
    opening.content = Arc::new(Cliff { rise: 1, hidden: 4 });
    opening
}

/// A [`Habit::Steady`] opening whose *origin* carries a field a capture does not
/// record.
///
/// Its very first mark is wrong, because the state a replay starts from is not
/// the state the run started from.
#[must_use]
pub(crate) fn opening_with_a_lossy_origin() -> Opening<Climb> {
    let mut opening = opening(Habit::Steady, 0, 0);
    // A whole new handle rather than a write through the old one. An `Arc` has
    // no `DerefMut`, so an opening's origin is replaced and never edited —
    // which is the same shape the level above it already had.
    opening.origin = Some(Arc::new(Climb {
        unwritten: 7,
        ..Climb::default()
    }));
    opening
}

/// Where a run of the climb that was told nothing else starts.
///
/// Every check here says which opening it wants, because what these fixtures
/// vary is the habit: which global a tick reads and when it starts reading it.
/// This is here because a [`Game`](corvid_app::Game) names a state that can
/// open a session on its own, and the steady habit is the honest answer for a
/// run nobody has configured.
impl Opens for Climb {
    fn opening() -> Opening<Self> {
        opening(Habit::Steady, 0, 0)
    }
}

/// The game the replay checks play: the climb, with nobody at the controls.
///
/// Four `()`s. What a replay check compares is a session against the states it
/// recorded, and neither a controller nor a device is on the path between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Climbing;

impl corvid_app::Game for Climbing {
    const PERIOD: corvid_time::TickSpan = corvid_time::TickSpan::CRADLE;

    type State = Climb;
    type Controller = ();
    type Bot = ();
    type Render = ();
    type Auralizer = ();
}

/// The input every run here plays with: nothing held, because this game's
/// `action` does not read one.
#[must_use]
pub(crate) fn idle() -> Input {
    Input::new(&[])
}
