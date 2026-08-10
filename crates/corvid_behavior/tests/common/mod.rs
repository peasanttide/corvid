//! The counter game: a complete `State` implementation, small enough to
//! read in one sitting and complete enough that the contract's guarantees can
//! be asserted against it rather than described.
//!
//! It counts. Each tick it adds the rules' step for every player who bumped,
//! records who those players were, and folds a joining player's profile into a
//! roster it carries forward. That is the smallest game that exercises all
//! three of the things the contract is for: a `State` with an allocation in it,
//! an `Action` whose default is genuinely idle, and a `Presence` the state has
//! to react to.
//!
//! The derives are behind `cfg_attr` the way a real game's would be: this crate
//! asks for an encoding only when its `serde` feature is on, so a game compiled
//! without one derives nothing and still implements the contract. That is the
//! arrangement being exercised rather than a detail of testing it.
//!
//! Both of the state's columns have to be reached for any of that to be
//! evidence. A session played entirely out of `Presence::Active` never folds a
//! profile in, so `roster` stays empty on every tick and a bug that emptied it
//! would look exactly like a correct run -- which is why [`run`] opens every
//! session on a tick the whole roster joins.

#![allow(
    dead_code,
    reason = "each integration test binary compiles this module separately, so anything only one of them uses is dead in the others"
)]

/// The vocabulary fixtures, which the two frozen tables share and the counter
/// game has nothing to do with.
pub(crate) mod vocabulary;

use std::sync::Arc;

use corvid_behavior::{
    Command, PlayerId, PlayerState, Presence, ProfileId, State as StateContract,
};

use corvid_time::Tick;

/// The level these tests open on.
pub(crate) const TERMINUS: &str = "terminus";

/// Authored and immutable within a session.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct Level {
    /// The name the runtime would have loaded this by.
    pub(crate) name: String,
    /// What the counter starts at when the session opens.
    pub(crate) start: i64,
}

/// Deterministic tuning every peer has to agree on.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct Rules {
    /// How far one bump moves the counter.
    pub(crate) step: i64,
}

/// Everything that cannot be recomputed quickly.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct State {
    /// The counter.
    pub(crate) count: i64,
    /// Who bumped it on the tick that produced this state, in the order the
    /// runtime handed the players over.
    pub(crate) movers: Vec<PlayerId>,
    /// Every profile that has ever joined, in join order.
    pub(crate) roster: Vec<ProfileId>,
}

/// One player's intent for one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) enum Action {
    /// The default, and what a dropped player submits forever.
    #[default]
    Idle,
    /// Move the counter by one step.
    Bump,
    /// Ask the runtime to quit, which is the only thing here that reaches
    /// outside the tick, and it reaches by describing rather than by doing.
    Leave,
}

/// Why this game could not read a level: it does not have one by that name.
///
/// A game's own error type, which is the whole of what the contract asks for.
/// This one has a single case because this game's levels are a fixed set; a
/// game that read files would put its filesystem's failures here instead.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("no level is called {0}")]
pub(crate) struct NoSuchLevel(String);

/// The levels this game has, by the names they answer to.
impl corvid_behavior::Level for Level {
    type Error = NoSuchLevel;

    fn load(name: &str) -> Result<Self, NoSuchLevel> {
        let start = match name {
            TERMINUS => 0,
            "arrival" => 100,
            _ => return Err(NoSuchLevel(name.to_owned())),
        };
        Ok(Self {
            name: name.to_owned(),
            start,
        })
    }
}

impl StateContract for State {
    const NAME: &'static str = "counter";

    type Level = Level;
    type Rules = Rules;
    type Action = Action;

    /// A new level restarts the counter at whatever the level says.
    ///
    /// The roster survives, because who is playing is not a property of the
    /// map they are playing on -- which is exactly the kind of thing only the
    /// game can decide, and the reason this method exists at all.
    fn load_level(self, _old: Option<&Level>, new: &Level) -> Self {
        Self {
            count: new.start,
            movers: Vec::new(),
            roster: self.roster,
        }
    }

    fn tick(
        self,
        level: &Level,
        players: &[PlayerState<Action>],
        rules: &Rules,
        command: &mut impl Command,
    ) -> Self {
        let mut count = self.count;

        let mut movers = Vec::new();
        for player in players {
            match player.action {
                Action::Idle => {}
                Action::Bump => {
                    count += rules.step;
                    movers.push(player.id);
                }
                Action::Leave => command.unload(&level.name),
            }
        }

        let mut roster = Vec::new();
        roster.extend_from_slice(&self.roster);
        for player in players {
            if let Presence::Joining { profile } = player.presence {
                roster.push(profile);
            }
        }

        Self {
            count,
            movers,
            roster,
        }
    }
}

/// The level every test opens on.
pub(crate) fn level() -> Arc<Level> {
    Arc::new(Level {
        name: TERMINUS.to_owned(),
        start: 0,
    })
}

/// The rules every test agrees on.
pub(crate) const RULES: Rules = Rules { step: 3 };

/// The state a session opens at.
pub(crate) const fn opening() -> State {
    State {
        count: 0,
        movers: Vec::new(),
        roster: Vec::new(),
    }
}

/// A roster of active players, ready to be given actions.
///
/// Identity comes from here -- from the runtime -- and not from anything the
/// game can read off an `Action`.
pub(crate) fn active(actions: &[Action]) -> Vec<PlayerState<Action>> {
    seats(actions, |_| Presence::Active)
}

/// The same seats on the one tick they arrive, each offering its profile.
///
/// Every session below opens with this rather than with a roster of actives,
/// and that is not decoration. `Presence::Active` is the only case `active`
/// emits, so a session built out of it alone leaves `State::roster` empty on
/// every tick of every run -- and a test that reads a state built that way is
/// reading one column of the two the game has. A dropped `roster` would be
/// invisible.
pub(crate) fn joining(actions: &[Action]) -> Vec<PlayerState<Action>> {
    seats(actions, |index| Presence::Joining {
        profile: ProfileId(1000 + u64::try_from(index).unwrap_or(0)),
    })
}

/// One seat per action, numbered from zero, with the presence this session is
/// at.
fn seats(actions: &[Action], presence: impl Fn(usize) -> Presence) -> Vec<PlayerState<Action>> {
    actions
        .iter()
        .enumerate()
        .map(|(index, action)| PlayerState {
            id: PlayerId(u16::try_from(index).unwrap_or(u16::MAX)),
            presence: presence(index),
            action: *action,
        })
        .collect()
}

/// Runs `ticks` ticks, dropping each retired state the way the runtime will,
/// and returns the final state and the tick it stopped at.
///
/// The first tick is the one every player joins on, so the roster column has
/// something in it from tick one onwards and stays non-empty for the rest of
/// the session.
pub(crate) fn run(ticks: usize, actions: &[Action]) -> (State, Tick) {
    let level = level();
    let arriving = joining(actions);
    let settled = active(actions);
    let mut state = opening();
    let mut tick = Tick::ZERO;

    for step in 0..ticks {
        let players = if step == 0 { &arriving } else { &settled };
        let next = state.clone().tick(
            &level,
            players,
            &RULES,
            &mut corvid_behavior::Discard::new(),
        );
        drop(core::mem::replace(&mut state, next));
        tick = tick.next();
    }

    (state, tick)
}
