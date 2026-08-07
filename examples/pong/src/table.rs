//! The simulation: five data types and one function, and no idea that a
//! network exists.
//!
//! Everything here is integer-only fixed point, on the XY plane in the
//! workspace's +X right, +Y forward, +Z up convention — so the court is drawn
//! from above and `z` is zero everywhere. What that buys is the whole claim
//! this example is for: two machines running this function over the same action
//! log reach the same bits, so a peer that guessed wrong about what the other
//! player did can be corrected by re-running it.

use std::{fmt, str::FromStr};

use corvid::{Command, FinePoint, I16F16, Player, PlayerId, State, Tick};
use serde::{Deserialize, Serialize};

/// The marker both halves of the game are implemented for.
///
/// The orphan rule wants one and it costs nothing: neither contract has a
/// method taking `self`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pong;

/// How this game names a level. There is one court.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Level {
    /// The court.
    Court,
}

impl Level {
    /// How it is spelled on a command line and in a save file.
    pub const COURT: &'static str = "court";
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Court => f.write_str(Self::COURT),
        }
    }
}

impl FromStr for Level {
    type Err = NoSuchLevel;

    /// ```
    /// use pong::Level;
    ///
    /// assert_eq!("court".parse(), Ok(Level::Court));
    /// assert!("stadium".parse::<Level>().is_err());
    /// ```
    ///
    /// # Errors
    ///
    /// [`NoSuchLevel`] for anything but `"court"`.
    fn from_str(name: &str) -> Result<Self, NoSuchLevel> {
        match name {
            Self::COURT => Ok(Self::Court),
            other => Err(NoSuchLevel {
                name: other.to_owned(),
            }),
        }
    }
}

/// This game has no level by that name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NoSuchLevel {
    /// What was asked for.
    pub name: String,
}

impl fmt::Display for NoSuchLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is not a level this game has; the only one is {}",
            self.name,
            Level::COURT,
        )
    }
}

impl std::error::Error for NoSuchLevel {}

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
    /// ball bounces off are at `±half.y()` and the goals are past `±half.x()`.
    pub half: FinePoint,
    /// How far in from `±half.x()` a paddle's face sits.
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
/// in the source, which is why `load` ignores the `Source` it is handed.
impl corvid::Level for Court {
    type Reference = Level;

    fn load(_reference: &Level, _files: &dyn corvid::Source) -> Result<Self, corvid::Malformed> {
        Ok(crate::court())
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
        players: &[Player<'_, Move>],
        rules: &Play,
        _command: &mut impl Command<Reference = Level>,
    ) -> Self {
        let mut table = self;
        table.now = table.now.next();
        table.contact = None;

        // A finished game still ticks — the session carries on, the digests
        // carry on agreeing, and the client draws the final score — but nothing
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

/// Moves each seated player's paddle, and clamps it to the court.
fn move_paddles(table: &mut Table, level: &Court, rules: &Play, players: &[Player<'_, Move>]) {
    let reach = level.reach();
    for player in players {
        let Some(paddle) = table.paddles.get_mut(usize::from(player.id.0)) else {
            // A roster longer than this game's two seats. Nothing to move, and
            // refusing to simulate would be a worse answer than ignoring a seat
            // that has no paddle.
            continue;
        };
        let step = match player.action {
            Move::Still => I16F16::ZERO,
            Move::Up => rules.paddle_speed,
            Move::Down => -rules.paddle_speed,
        };
        paddle.at = (paddle.at + step).clamp(-reach, reach);
    }
}

/// Puts the ball at the centre and sends it towards whoever is receiving.
///
/// The vertical component alternates with the serve direction rather than
/// coming from a random number, because this game has no randomness in it at
/// all: a session's whole outcome is the two action logs, which is what makes a
/// desync report unambiguous when one happens.
fn serve(table: &mut Table, rules: &Play) {
    let across = if table.towards {
        rules.serve_speed
    } else {
        -rules.serve_speed
    };
    let lift = if table.now.0.is_multiple_of(2) {
        rules.serve_lift
    } else {
        -rules.serve_lift
    };
    table.ball = Ball {
        at: FinePoint::new(I16F16::ZERO, I16F16::ZERO, I16F16::ZERO),
        velocity: FinePoint::new(across, lift, I16F16::ZERO),
    };
}

/// Reflects the ball off the top and the bottom of the court.
///
/// Reflection rather than a clamp, for the reason `examples/hello` gives: a
/// ball that overshot the wall by a centimetre comes back a centimetre inside
/// it, where a clamp would quietly delete the overshoot and make the bounce a
/// function of the frame it happened on.
fn bounce_off_walls(table: &mut Table, level: &Court) {
    let limit = level.half.y() - level.ball;
    let y = table.ball.at.y();
    let wall = if y > limit {
        limit
    } else if y < -limit {
        -limit
    } else {
        return;
    };
    let [x, _, z] = table.ball.at.to_array();
    let bounced = wall + wall - y;
    table.ball.at = FinePoint::new(x, bounced, z);
    let [vx, vy, vz] = table.ball.velocity.to_array();
    table.ball.velocity = FinePoint::new(vx, -vy, vz);
    table.contact = Some(Contact::Wall { at: table.ball.at });
}

/// Plays the ball off a paddle it crossed this tick.
///
/// **The test is a crossing rather than an overlap**, and that is what stops a
/// fast ball from passing through a paddle: the ball moves up to
/// [`Play::top_speed`] in one tick, which is further than a paddle is thick, so
/// a check that asked whether the ball is *inside* the paddle now would miss
/// every fast shot. What is asked instead is whether the segment from where the
/// ball was to where it is crosses the paddle's face while the paddle covers
/// it.
fn play_off_paddles(table: &mut Table, level: &Court, rules: &Play, was: FinePoint) {
    for seat in 0..SEATS {
        let face = level.face(seat);
        let (from, to) = (was.x(), table.ball.at.x());
        // Towards the paddle's own end, and across its face. A ball travelling
        // away from an end cannot be played by it, which is what keeps a ball
        // that has just been hit from being hit again on the next tick.
        let crossed = if seat == 0 {
            from >= face && to <= face
        } else {
            from <= face && to >= face
        };
        if !crossed {
            continue;
        }

        let Some(paddle) = table.paddles.get(seat) else {
            continue;
        };
        // Where the ball is when it reaches the face, along `y`. Interpolating
        // would be more exact and would cost a division; at these speeds the
        // ball travels less than its own width across the paddle's thickness,
        // so the position after the step is inside the tolerance the paddle's
        // half-height already is.
        let offset = table.ball.at.y() - paddle.at;
        if offset.abs() > level.paddle.y() + level.ball {
            continue;
        }

        // Reflected about the face, so a ball that went a little past comes
        // back the same distance in front of it.
        let bounced = face + face - table.ball.at.x();
        let [_, y, z] = table.ball.at.to_array();
        table.ball.at = FinePoint::new(bounced, y, z);

        let [vx, vy, vz] = table.ball.velocity.to_array();
        let faster = (vx.abs() + rules.speed_up).min(rules.top_speed);
        let across = if seat == 0 { faster } else { -faster };
        // The whole of a player's control over where the ball goes: hitting it
        // with the edge of the paddle sends it away from the paddle's centre.
        let lift = vy + offset * rules.spin;
        table.ball.velocity =
            FinePoint::new(across, lift.clamp(-rules.top_speed, rules.top_speed), vz);
        table.contact = Some(Contact::Paddle {
            at: table.ball.at,
            seat: u8::try_from(seat).unwrap_or(u8::MAX),
        });
        return;
    }
}

/// Scores a ball that went past a paddle, and starts the next serve.
fn score(table: &mut Table, level: &Court, rules: &Play) {
    let x = table.ball.at.x();
    let scorer = if x < -level.half.x() {
        1
    } else if x > level.half.x() {
        0
    } else {
        return;
    };

    if let Some(score) = table.scores.get_mut(scorer) {
        *score = score.saturating_add(1);
        if *score >= rules.target {
            table.over = u8::try_from(scorer).ok();
        }
    }
    // Towards whoever was just scored on, which is the convention every pong
    // has: the player who lost the point receives.
    table.towards = scorer == 0;
    table.serve = level.serve;
    table.ball = Ball {
        at: FinePoint::new(I16F16::ZERO, I16F16::ZERO, I16F16::ZERO),
        velocity: FinePoint::new(I16F16::ZERO, I16F16::ZERO, I16F16::ZERO),
    };
    table.contact = Some(Contact::Goal {
        seat: u8::try_from(scorer).unwrap_or(u8::MAX),
    });
}

/// Which seat a player is, as an index into the two-long arrays here.
///
/// A free function rather than a method on [`PlayerId`] because the mapping is
/// this game's: seat zero defends `-x`, and a roster longer than two has seats
/// this game cannot draw a paddle for.
#[must_use]
pub const fn index(seat: PlayerId) -> usize {
    seat.0 as usize
}
