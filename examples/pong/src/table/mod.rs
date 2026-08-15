//! The simulation: five data types and one function, and no idea that a
//! network exists.
//!
//! Everything here is integer-only fixed point, on the XY plane in the
//! workspace's +X right, +Y forward, +Z up convention -- so the court is drawn
//! from above and `z` is zero everywhere. What that buys is the whole claim
//! this example is for: two machines running this function over the same action
//! log reach the same bits, so a peer that guessed wrong about what the other
//! player did can be corrected by re-running it.

use corvid::{Command, FinePoint, I16F16, PlayerState, State, Tick};
use serde::{Deserialize, Serialize};

/// How this game names its one level, on a command line and in a save file.
///
/// A `&str` because [`Level::load`](corvid::Level::load) reads one: a game with
/// a fixed set of levels matches on the name, and this one has a set of one.
pub const COURT: &str = "court";

/// This game has no level by that name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[error("{name} is not a level this game has; the only one is {COURT}")]
pub struct NoSuchLevel {
    /// What was asked for.
    pub name: String,
}

/// How many play. Two, and every array here is this long.
pub const SEATS: usize = 2;

/// Authored, immutable within a session, hashed into the opening.
///
/// The shape of the court, in metres. Everything the simulation compares a
/// position against is here rather than a literal in [`Table::tick`], because a
/// client draws the same court and the two must be the same numbers.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Court {
    /// Half the court's width in `x` and half its height in `y`. The walls the
    /// ball bounces off are at `+/-half.y()` and the goals are past `+/-half.x()`.
    pub half: FinePoint,
    /// How far in from `+/-half.x()` a paddle's face sits.
    pub inset: I16F16,
    /// Half a paddle's height and half its width.
    pub paddle: FinePoint,
    /// Half the ball's width.
    pub ball: I16F16,
    /// How many ticks the ball waits at the centre after a goal.
    pub serve: u16,
}

impl Court {
    /// Where the face of one seat's paddle is, in `x`.
    ///
    /// Seat zero defends `-x` and seat one defends `+x`, which is the whole of
    /// what a seat number means in this game.
    #[must_use]
    pub fn face(&self, seat: usize) -> I16F16 {
        let from_wall = self.half.x() - self.inset;
        if seat == 0 { -from_wall } else { from_wall }
    }

    /// Where the centre of one seat's paddle is, in `x`.
    ///
    /// Half a paddle's thickness *behind* [`face`](Self::face), away from the
    /// middle of the court, so that the face is the paddle's court-facing edge
    /// rather than a line through the middle of it.
    ///
    /// This is what a client draws the paddle around, and it exists because
    /// getting it wrong is invisible to every test and obvious to every player:
    /// drawing the rectangle centred on `face` puts its near edge
    /// [`paddle.x()`](Self::paddle) closer to the middle than the plane the
    /// ball actually reflects off, so the ball sinks into the sprite by its own
    /// width plus the paddle's before it comes back.
    #[must_use]
    pub fn centre(&self, seat: usize) -> I16F16 {
        let face = self.face(seat);
        if seat == 0 {
            face - self.paddle.x()
        } else {
            face + self.paddle.x()
        }
    }

    /// The furthest a paddle's centre may travel from the middle before it
    /// would leave the court.
    #[must_use]
    pub fn reach(&self) -> I16F16 {
        (self.half.y() - self.paddle.y()).max(I16F16::ZERO)
    }
}

/// Deterministic tuning. Every peer agrees on it, and it feeds the hash.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Play {
    /// How far a paddle moves in one tick when its player is holding a
    /// direction.
    pub paddle_speed: I16F16,
    /// How fast the ball leaves a serve, along `x`.
    pub serve_speed: I16F16,
    /// What the ball's vertical speed is on a serve, before any paddle has
    /// touched it.
    pub serve_lift: I16F16,
    /// What a paddle hit adds to the ball's horizontal speed.
    pub speed_up: I16F16,
    /// The fastest the ball may travel along `x` in one tick, however many
    /// times it has been hit. Without it a long rally ends in a ball that
    /// crosses the court in one tick and cannot be played.
    pub top_speed: I16F16,
    /// How much of the distance between the ball and the paddle's centre
    /// becomes vertical speed. This is the whole of a player's control over
    /// where the ball goes.
    pub spin: I16F16,
    /// How many goals wins.
    pub target: u16,
}

/// One paddle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Paddle {
    /// Where its centre is along `y`.
    pub at: I16F16,
}

/// The ball.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Ball {
    /// Where it is. `z` is zero and stays there.
    pub at: FinePoint,
    /// How far it travels in one tick.
    pub velocity: FinePoint,
}

/// What happened on the tick that produced a state.
///
/// In the state rather than worked out by a client, because a hit is a
/// simulation event: every peer agrees on it, it survives a save, and a client
/// that recomputed it from two positions would have to guess. It is what the
/// sound and the flash are read out of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Contact {
    /// The ball reached the top or the bottom of the court.
    Wall {
        /// Where it touched.
        at: FinePoint,
    },
    /// A paddle played it.
    Paddle {
        /// Where it touched.
        at: FinePoint,
        /// Whose paddle.
        seat: u8,
    },
    /// It went past a paddle, and the other seat scored.
    Goal {
        /// Who scored.
        seat: u8,
    },
}

/// Everything that cannot be recomputed: serialized, hashed, rolled back.
///
/// Small on purpose. A reader following a rollback trace can hold the whole of
/// this in their head, which is the reason the game under this netcode is pong
/// and not a tower defence.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Table {
    /// The ball.
    pub ball: Ball,
    /// The paddles, seat zero first.
    pub paddles: [Paddle; SEATS],
    /// The score, seat zero first.
    pub scores: [u16; SEATS],
    /// How many ticks until the ball is served, and zero while it is in play.
    pub serve: u16,
    /// Which way the next serve travels: `false` towards `-x`, `true` towards
    /// `+x`. It is in the state rather than derived from the score so that a
    /// client and a peer cannot disagree about it.
    pub towards: bool,
    /// What happened on the tick that produced this state.
    pub contact: Option<Contact>,
    /// Which tick this state is at.
    ///
    /// A tick is not handed its own number, so a game that wants one counts. It
    /// is also what [`App::until`](corvid::App::until) reads.
    pub now: Tick,
    /// The seat that reached [`Play::target`], once one has.
    pub over: Option<u8>,
}

/// One player's intent for one tick. Goes on the wire; `Default` is idle.
///
/// A direction rather than a position, and that is a netcode decision as much
/// as a design one: prediction repeats a seat's newest action, so a player
/// holding [`Up`](Move::Up) for twenty ticks is predicted right nineteen times
/// and the tick they change direction is the tick that mispredicts. A paddle
/// *position* on the wire would be right by accident every tick and would make
/// this example prove nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Move {
    /// Hands off, and what a seat nobody is sitting in submits forever.
    #[default]
    Still,
    /// Towards `+y`.
    Up,
    /// Towards `-y`.
    Down,
}

/// The court is a constant, so it reads nothing.
///
/// A game whose levels come off a disk reads them here; this one's is written
/// in the source, so the whole of `load` is matching the name against the one
/// this game has.
impl corvid::Level for Court {
    type Error = NoSuchLevel;

    fn load(name: &str) -> Result<Self, NoSuchLevel> {
        if name == COURT {
            Ok(crate::court())
        } else {
            Err(NoSuchLevel {
                name: name.to_owned(),
            })
        }
    }
}

impl State for Table {
    const NAME: &'static str = "pong";

    type Level = Court;
    type Rules = Play;
    type Action = Move;

    /// One tick: the paddles move, the ball moves, and whatever it touched is
    /// written down.
    ///
    /// A pure function of what its arguments denote, which is what makes two
    /// machines agree and what makes a rollback able to recompute a stretch of
    /// ticks from a snapshot. Nothing here reads a clock, a random number, or
    /// anything about which machine it is running on.
    fn tick(
        self,
        level: &Court,
        players: &[PlayerState<Move>],
        rules: &Play,
        _command: &mut impl Command,
    ) -> Self {
        let mut table = self;
        table.now = table.now.next();
        table.contact = None;

        // A finished game still ticks -- the session carries on, the digests
        // carry on agreeing, and the client draws the final score -- but nothing
        // in it moves. A run that wants to stop when somebody wins says so with
        // `App::until`, which is the runtime's business rather than the
        // simulation's.
        if table.over.is_some() {
            return table;
        }

        move_paddles(&mut table, level, rules, players);

        if table.serve > 0 {
            table.serve -= 1;
            if table.serve == 0 {
                serve(&mut table, rules);
            }
            return table;
        }

        let was = table.ball.at;
        table.ball.at += table.ball.velocity;
        bounce_off_walls(&mut table, level);
        play_off_paddles(&mut table, level, rules, was);
        score(&mut table, level, rules);

        table
    }
}

mod tick;

pub use tick::index;

use tick::{bounce_off_walls, move_paddles, play_off_paddles, score, serve};
