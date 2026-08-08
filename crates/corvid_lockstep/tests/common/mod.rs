//! The stand-in game every test here plays.
//!
//! A struct-of-arrays state sorted by region, which is the shape a crowd of
//! creeps has, with the row count set by the level — so the same game is four
//! rows in a prediction test and fifty thousand in the budget one, and the
//! measurement is of the same code the rest of the file asserts about.

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
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "the opening's columns are filled from a row index the level's own row count bounds, and a wrapped one would still be a deterministic column"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "this module is private to each test binary, so pub(crate) and pub are equivalent — pub(crate) is the one rustc's unreachable_pub asks for, and the two lints cannot both be satisfied"
)]

use std::sync::Arc;

use corvid_behavior::PlayerId;

use corvid_hash::Digest;

use corvid_lockstep::{Bisect, Budget, Datagram, Peer, Probes, Where};

use corvid_behavior::{Command, Player, ProfileId, State as StateContract};
use corvid_replay::Session;
use corvid_replay::{Opening, Profile, Schema, Seed};
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

/// Authored and immutable within a session.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Level {
    /// How many creeps the level starts with.
    pub(crate) rows: u32,
    /// How far a creep may travel before it wraps.
    pub(crate) ceiling: i32,
}

/// Deterministic tuning every peer has to agree on.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Rules {
    /// How sharply a creep sheds velocity, as a shift.
    pub(crate) drag: u32,
}

/// Everything that cannot be recomputed quickly, in struct-of-arrays form and
/// sorted by region.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Swarm {
    /// One creep's position per row.
    pub(crate) position: Vec<i32>,
    /// One creep's velocity per row.
    pub(crate) velocity: Vec<i32>,
    /// One tower's charge per row.
    pub(crate) towers: Vec<i32>,
    /// Which region of the level each row is in.
    pub(crate) region: Vec<u16>,
    /// A mix of every action every tick has ever been handed.
    ///
    /// A position column alone is a poor fixture for a rollback: it wraps, so a
    /// correction far enough back washes out. This column never forgets, so a
    /// change anywhere in the history is visible at every tick after it.
    pub(crate) folded: u64,
}

/// One player's intent for one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum Action {
    /// The default, and what a dropped player submits forever.
    #[default]
    Idle,
    /// Shove every creep along.
    Push {
        /// How hard.
        force: i16,
    },
    /// Charge every tower.
    Build,
}

/// The level reads nothing: this fixture's is a constant.
impl corvid_behavior::Level for Level {
    type Reference = String;

    fn load(
        _reference: &String,
        _files: &dyn corvid_files::Source,
    ) -> Result<Self, corvid_files::Malformed> {
        Ok(Self {
            rows: 0,
            ceiling: 0,
        })
    }
}

impl StateContract for Swarm {
    const NAME: &'static str = "swarm";

    type Level = Level;
    type Rules = Rules;
    type Action = Action;

    fn tick(
        self,
        _level: &Level,
        players: &[Player<'_, Action>],
        rules: &Rules,
        _command: &mut impl Command<Reference = String>,
    ) -> Self {
        let previous = &self;
        let mut folded = previous.folded;
        let mut force = 0_i32;
        let mut charge = 0_i32;
        for player in players {
            let code = match *player.action {
                Action::Idle => 1,
                Action::Push { force: by } => {
                    force = force.wrapping_add(i32::from(by));
                    2_u64.wrapping_add(u64::from(by.unsigned_abs()) << 8)
                }
                Action::Build => {
                    charge = charge.wrapping_add(1);
                    3
                }
            };
            folded = folded
                .wrapping_mul(0x0100_0000_01b3)
                .wrapping_add(u64::from(player.id.0) << 32)
                .wrapping_add(code);
        }

        let rows = previous.position.len();
        let mut position = Vec::with_capacity(rows);
        let mut velocity = Vec::with_capacity(rows);
        let mut towers = Vec::with_capacity(rows);
        let mut region = Vec::with_capacity(rows);
        let drag = rules.drag & 31;

        for (((was, moving), tower), home) in previous
            .position
            .iter()
            .zip(&previous.velocity)
            .zip(&previous.towers)
            .zip(&previous.region)
        {
            let speed = moving.wrapping_add(force).wrapping_sub(moving >> drag);
            velocity.push(speed);
            position.push(was.wrapping_add(speed));
            towers.push(tower.wrapping_add(charge));
            region.push(*home);
        }

        Self {
            position,
            velocity,
            towers,
            region,
            folded,
        }
    }
}

impl Bisect for Swarm {
    fn probe(state: &Self, out: &mut Probes) {
        out.column("state.creeps.position", &state.position);
        out.column("state.creeps.velocity", &state.velocity);
        out.column("state.towers", &state.towers);
    }

    fn locate(state: &Self, probe: &str, remote: &[Digest]) -> Option<Where> {
        let mut probes = Probes::default();
        Self::probe(state, &mut probes);
        let index = probes.locate(probe, remote)?;
        let region = state
            .region
            .get(usize::try_from(index).ok()?)
            .copied()
            .unwrap_or_default();
        Some(Where {
            probe: "creep",
            index,
            region,
        })
    }
}

/// The state a level of `rows` creeps opens on.
pub(crate) fn origin(rows: u32) -> Swarm {
    let rows = usize::try_from(rows).unwrap();
    Swarm {
        position: (0..rows).map(|row| row as i32).collect(),
        velocity: (0..rows).map(|row| (row % 7) as i32).collect(),
        towers: (0..rows).map(|row| (row % 3) as i32).collect(),
        region: (0..rows).map(|row| (row / 1_024) as u16).collect(),
        folded: 0x9e37_79b9_7f4a_7c15,
    }
}

/// A session of `rows` creeps and `seats` players, opening at tick zero.
pub(crate) fn session(rows: u32, seats: u16) -> Session<Swarm> {
    let opening = Opening::<Swarm> {
        level: "terminus".to_owned(),
        content: Arc::new(Level {
            rows,
            ceiling: 1 << 20,
        }),
        rules: Arc::new(Rules { drag: 4 }),
        roster: (0..seats)
            .map(|seat| Profile {
                account: ProfileId(u64::from(seat) + 1),
                joined: Tick::ZERO,
                left: None,
            })
            .collect(),
        seed: Seed(0x5eed),
        first: Tick::ZERO,
        origin: Some(Arc::new(origin(rows))),
        schema: Schema::new("swarm")
            .field("Swarm.position", "Vec<i32>")
            .field("Swarm.velocity", "Vec<i32>")
            .digest(),
    };
    Session::new(opening).unwrap()
}

/// One peer of a session of `rows` creeps and `seats` players.
pub(crate) fn peer(rows: u32, seats: u16, seat: u16, budget: Budget) -> Peer<Swarm> {
    Peer::new(session(rows, seats), PlayerId(seat), budget)
}

/// A push of a given force, which is the action a mispredict is made of.
pub(crate) const fn push(force: i16) -> Action {
    Action::Push { force }
}

/// A datagram a seat would have sent, assembled by hand.
///
/// The tests hand these from one peer to another by value, which is the whole
/// of the transport this crate needs. `mark` is the sender's digest at the
/// opening, which every peer of the same session agrees about — a datagram
/// carrying a mark nobody could have computed would be a desync rather than a
/// fixture.
pub(crate) fn beat(
    seat: u16,
    head: u64,
    actions: [Action; corvid_lockstep::WINDOW],
    mark: Digest,
) -> Datagram<Action> {
    // The window ends at `head`, so its first row is `WINDOW - 1` ticks before
    // it — saturating at the opening, which is what a datagram sent in a
    // session's first ticks carries.
    let span = corvid_lockstep::WINDOW as u64 - 1;
    Datagram {
        seat: PlayerId(seat),
        first: Tick(head.saturating_sub(span)),
        actions: actions.to_vec(),
        heard: None,
        marked: Tick::ZERO,
        mark,
    }
}
