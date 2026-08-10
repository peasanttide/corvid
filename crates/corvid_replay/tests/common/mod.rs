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
    reason = "this module is private to each test binary, so pub(crate) and pub are equivalent — pub(crate) is the one rustc's unreachable_pub asks for, and the two lints cannot both be satisfied"
)]

use std::sync::Arc;

use corvid_behavior::{
    Command, Level as LevelContract, PlayerId, PlayerState, Presence, ProfileId,
    State as StateContract,
};
use corvid_hash::{Digest, digest};
use corvid_replay::{ActionLog, HashTrace, Opening, Profile, Schema, Seed, Session};
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

/// The level every session here opens on.
pub(crate) const TERMINUS: &str = "terminus";

/// The counter game, named by its state.
///
/// The state *is* the game now — there is no marker type — so this alias is
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

/// The level reads its ceiling out of its own name, which is where a fixture's
/// levels live now that the contract hands over a name and nothing else.
///
/// `"counter-40"` is a ceiling of forty. A game that read files would open one
/// here instead, and the contract would look the same either way.
impl LevelContract for Level {
    type Error = core::convert::Infallible;

    fn load(name: &str) -> Result<Self, core::convert::Infallible> {
        let ceiling = name
            .rsplit_once('-')
            .and_then(|(_, digits)| digits.parse().ok())
            .unwrap_or(0);
        Ok(Self {
            name: name.to_owned(),
            ceiling,
        })
    }
}

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

/// This build's description of its own types.
pub(crate) fn schema() -> Digest {
    Schema::new("counter")
        .field("State.count", "i64")
        .field("State.folded", "u64")
        .field("State.movers", "Vec<PlayerId>")
        .field("State.roster", "Vec<ProfileId>")
        .digest()
}

/// The roster these sessions play with: four seats, one of which arrives late
/// and one of which leaves partway through.
///
/// Both of those are here so that presence is exercised rather than assumed. A
/// roster where everybody joins at tick zero and nobody leaves makes
/// `Presence::Active` the only value a replay ever has to reconstruct.
pub(crate) fn roster() -> Vec<Profile> {
    vec![
        Profile {
            account: ProfileId(11),
            joined: Tick::ZERO,
            left: None,
        },
        Profile {
            account: ProfileId(22),
            joined: Tick::ZERO,
            left: Some(Tick(300)),
        },
        Profile {
            account: ProfileId(33),
            joined: Tick(7),
            left: None,
        },
        Profile {
            account: ProfileId(44),
            joined: Tick(120),
            left: None,
        },
    ]
}

/// An opening for that roster.
pub(crate) fn opening() -> Opening<Counter> {
    Opening {
        level: TERMINUS.to_owned(),
        content: Arc::new(Level {
            name: TERMINUS.to_owned(),
            ceiling: 10_000,
        }),
        rules: Arc::new(Rules { step: 3 }),
        roster: roster(),
        seed: Seed(0x5eed_0000_0000_0001),
        first: Tick::ZERO,
        // Deliberately not `None`, and deliberately not `State::default()`
        // either: an opening whose origin is the default would not tell a
        // resolved origin from an absent one, and every seek in these tests
        // starts from this state.
        origin: Some(Arc::new(State {
            count: 1,
            folded: 0x0bad_c0de,
            movers: Vec::new(),
            roster: Vec::new(),
        })),
        schema: schema(),
    }
}

/// How many seats a session's roster has, for the tests that fill the log a row
/// at a time.
///
/// [`Opening::seats`] answers [`None`] for a roster no `PlayerId` could name, a
/// shape no fixture here builds: the tests that are about that roster build it
/// by hand and assert the refusal rather than asking for a width.
pub(crate) fn seats(session: &Session<Counter>) -> u16 {
    session
        .opening
        .seats()
        .expect("every fixture roster here fits in a u16")
}

/// What seat `player` did on tick `tick`, deterministically and without a
/// pattern a bug could accidentally satisfy.
pub(crate) fn scripted(tick: Tick, player: PlayerId) -> Action {
    let mixed = tick
        .0
        .wrapping_mul(0x9e37_79b9_7f4a_7c15)
        .wrapping_add(u64::from(player.0).wrapping_mul(0xbea2_25f9_eb34_556d));
    match (mixed >> 47) % 8 {
        0 => Action::Reset,
        1..=3 => Action::Idle,
        _ => Action::Bump,
    }
}

/// A session whose log holds `ticks` rows of [`scripted`] actions.
pub(crate) fn play(ticks: u64) -> Session<Counter> {
    let mut session = Session::new(opening()).expect("four seats fit in a u16");
    if ticks == 0 {
        return session;
    }
    session
        .log
        .extend_to(Tick(ticks - 1))
        .expect("the log grows from its own first tick");
    for tick in 0..ticks {
        for seat in 0..seats(&session) {
            let player = PlayerId(seat);
            session
                .log
                .set(Tick(tick), player, scripted(Tick(tick), player))
                .expect("a fresh log has nothing confirmed to contradict");
        }
    }
    session.marks = forward(&session).1;
    session
}

/// Runs a session forward from its opening, by hand.
///
/// Returns the state at every tick and the trace of their digests. This is the
/// answer `seek` is checked against, and it shares no code with `seek`.
///
/// The states come back by value rather than behind the handles a seek returns.
/// That is the second half of the same independence: a comparison against this
/// dereferences whatever `seek` produced, so it is a comparison of two states
/// and never of two pointers into the same allocation.
pub(crate) fn forward(session: &Session<Counter>) -> (Vec<State>, HashTrace) {
    let mut state = (*session.opening.origin()).clone();
    let mut marks = HashTrace::new(session.opening.first);
    let mut states = vec![state.clone()];
    marks.push(digest(&state));

    let idle = Action::default();
    let mut at = session.opening.first;
    while at < session.last() {
        let players: Vec<PlayerState<Action>> = session
            .opening
            .roster
            .iter()
            .enumerate()
            .filter_map(|(seat, profile)| {
                let id = PlayerId(u16::try_from(seat).expect("four seats fit in a u16"));
                Some(PlayerState {
                    id,
                    presence: profile.presence_at(at)?,
                    action: *session.log.get(at, id).unwrap_or(&idle),
                })
            })
            .collect();

        state = state.tick(
            &session.opening.content,
            &players,
            &session.opening.rules,
            &mut corvid_behavior::Discard::new(),
        );
        states.push(state.clone());
        marks.push(digest(&state));
        at = at.next();
    }

    (states, marks)
}

/// A log nobody has contradicted, for the tests that are about what the ring
/// charges and evicts rather than about which history a state came from.
///
/// [`Snapshots::keep`](corvid_replay::Snapshots::keep) and
/// [`Snapshots::nearest`](corvid_replay::Snapshots::nearest) take a log because
/// a snapshot is keyed to the generation of the one that produced it. A log that
/// has taken no corrections reports generation zero for every tick, so passing
/// this leaves those tests measuring exactly what they measured before the
/// generation existed.
pub(crate) const fn quiet_log() -> ActionLog<Action> {
    ActionLog::new(Tick::ZERO, 4)
}

/// A log's worth of actions, as a fixture for the golden tables.
pub(crate) fn small_log() -> ActionLog<Action> {
    let mut log = ActionLog::new(Tick(4), 2);
    log.extend_to(Tick(5)).expect("tick 5 is after tick 4");
    log.set(Tick(4), PlayerId(1), Action::Bump)
        .expect("nothing is confirmed yet");
    log.set(Tick(5), PlayerId(0), Action::Reset)
        .expect("nothing is confirmed yet");
    log
}

/// The roster the two golden tables are recorded over: one seat still playing
/// and one that joined late and left, so that both shapes of
/// [`Profile::left`] appear in the rows.
pub(crate) fn golden_roster() -> Vec<Profile> {
    vec![
        Profile {
            account: ProfileId(0x11),
            joined: Tick(4),
            left: None,
        },
        Profile {
            account: ProfileId(0x22),
            joined: Tick(5),
            left: Some(Tick(6)),
        },
    ]
}

/// The trace the two golden tables are recorded over.
pub(crate) fn golden_trace() -> HashTrace {
    let mut trace = HashTrace::new(Tick(4));
    trace.push(Digest::from_u64(0x1111_2222_3333_4444));
    trace.push(Digest::from_u64(0x5555_6666_7777_8888));
    trace
}

/// The opening the two golden tables are recorded over.
///
/// Every field holds a different small number, so that a row moves when two
/// fields are exchanged rather than staying still because both happened to hold
/// the same value.
pub(crate) fn golden_opening() -> Opening<Counter> {
    Opening {
        level: TERMINUS.to_owned(),
        content: Arc::new(Level {
            name: TERMINUS.to_owned(),
            ceiling: 7,
        }),
        rules: Arc::new(Rules { step: 2 }),
        roster: golden_roster(),
        seed: Seed(0x0102_0304_0506_0708),
        first: Tick(4),
        origin: Some(Arc::new(State {
            count: 5,
            folded: 6,
            movers: vec![PlayerId(1)],
            roster: vec![ProfileId(9)],
        })),
        schema: Digest::from_u64(0x0a0b_0c0d_0e0f_1011),
    }
}

/// The whole session the two golden tables are recorded over.
pub(crate) fn golden_session() -> Session<Counter> {
    Session {
        opening: golden_opening(),
        log: small_log(),
        marks: golden_trace(),
    }
}
