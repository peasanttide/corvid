//! The counter game these tests replay, and a forward run to compare a seek
//! against.
//!
//! The forward run is written out by hand rather than expressed as a seek. That
//! is the whole point of it: a test that compared `seek` against `seek` would
//! agree with itself about a roster built the wrong way round or a row read one
//! tick late, and the two implementations here are independent enough that
//! either of those shows up as a difference.

#![allow(
    dead_code,
    reason = "each integration test binary compiles this module separately, so anything only one of them uses is dead in the others"
)]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "this module is private to each test binary, so pub(crate) and pub are equivalent -- pub(crate) is the one rustc's unreachable_pub asks for, and the two lints cannot both be satisfied"
)]

use corvid_behavior::{
    Command, Level as LevelContract, PlayerId, PlayerState, Presence, ProfileId,
    State as StateContract,
};
use serde::{Deserialize, Serialize};

/// The level every session here opens on.
pub(crate) const TERMINUS: &str = "terminus";

/// The counter game, named by its state.
///
/// The state *is* the game now -- there is no marker type -- so this alias is
/// what keeps `Opening<Counter>` reading as "an opening for the counter game"
/// rather than as an opening for a struct called `State`.
pub(crate) type Counter = State;

/// Authored and immutable within a session.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Level {
    /// The name the runtime would have loaded this by.
    pub(crate) name: String,
    /// The most the counter may reach.
    pub(crate) ceiling: i64,
}

/// Deterministic tuning every peer has to agree on.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Rules {
    /// How far one bump moves the counter.
    pub(crate) step: i64,
}

/// Everything that cannot be recomputed quickly.
///
/// Three columns rather than one, because a state with a single integer in it
/// would hash the same however wrong the roster was.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct State {
    /// The counter.
    pub(crate) count: i64,
    /// A mix of every action every tick has ever been handed.
    ///
    /// The counter alone is a poor fixture for a rollback: `Reset` puts it back
    /// to zero, so a correction five ticks back is washed out by the time the
    /// re-simulation catches up and a test that compared counts would be
    /// comparing two runs that genuinely agree. This column never forgets, so a
    /// change anywhere in the history is visible at every tick after it.
    pub(crate) folded: u64,
    /// Who moved it on the tick that produced this state.
    pub(crate) movers: Vec<PlayerId>,
    /// Every profile that has ever joined, in join order. This is the column
    /// that only moves on a `Presence::Joining` tick, so a replay that
    /// reconstructed presence wrongly shows up here and nowhere else.
    pub(crate) roster: Vec<ProfileId>,
}

/// One player's intent for one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum Action {
    /// The default, and what a dropped player submits forever.
    #[default]
    Idle,
    /// Move the counter by one step.
    Bump,
    /// Put the counter back to zero, so that the state is a function of the
    /// whole history rather than of a sum that any order of actions reaches.
    Reset,
}

/// The level derives its ceiling from its own name.
///
/// Since #27 a level is named by a `&str` and loads itself however it likes --
/// this one has a fixed set and needs no filesystem, which is the case that
/// motivated dropping the `Source` argument.
impl LevelContract for Level {
    type Error = UnknownLevel;

    fn load(name: &str) -> Result<Self, Self::Error> {
        let ceiling = match name {
            TERMINUS => 7,
            "shallow" => 2,
            _ => return Err(UnknownLevel(String::from(name))),
        };
        Ok(Self {
            name: String::from(name),
            ceiling,
        })
    }
}

/// The only way loading a level can fail here: a name this game does not have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UnknownLevel(pub(crate) String);

impl core::fmt::Display for UnknownLevel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "no level named {:?}", self.0)
    }
}

impl core::error::Error for UnknownLevel {}

impl StateContract for State {
    const NAME: &'static str = "counter";

    type Level = Level;
    type Rules = Rules;
    type Action = Action;

    fn tick(
        self,
        level: &Level,
        players: &[PlayerState<Action>],
        rules: &Rules,
        _command: &mut impl Command,
    ) -> Self {
        let mut count = self.count;
        let mut folded = self.folded;
        // A fresh column every tick, here and for the roster below. A state
        // owns what it hands out and may outlive this tick behind any number of
        // handles, so there is nothing a pool could take back.
        let mut movers = Vec::new();

        for player in players {
            folded = folded
                .wrapping_mul(0x0100_0000_01b3)
                .wrapping_add(u64::from(player.id.0) << 8)
                .wrapping_add(match player.action {
                    Action::Idle => 1,
                    Action::Bump => 2,
                    Action::Reset => 3,
                });
            match player.action {
                Action::Idle => {}
                Action::Bump => {
                    count = (count + rules.step).min(level.ceiling);
                    movers.push(player.id);
                }
                Action::Reset => {
                    count = 0;
                    movers.push(player.id);
                }
            }
        }

        // Moved out of the state this tick consumed rather than cloned, which
        // is the allocation reuse `self` by value replaced `Scratch` with.
        let mut roster = self.roster;
        for player in players {
            if let Presence::Joining { profile } = player.presence {
                roster.push(profile);
            }
        }

        Self {
            count,
            folded,
            movers,
            roster,
        }
    }
}

mod fixtures;

pub(crate) use fixtures::*;
