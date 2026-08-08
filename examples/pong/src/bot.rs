//! An opponent that is actually trying.
//!
//! A paddle that follows the ball's *current* height is a paddle that is always
//! where the ball was: it arrives late to anything hit hard and misses anything
//! hit at an angle. What this does instead is what a person does — work out
//! where the ball is going to *be* when it reaches this end, go there, and
//! decide which part of the paddle to meet it with.
//!
//! Everything here is a pure function of the state and the court, which is
//! worth saying because it is what lets the same code drive a peer in the
//! netcode lab, the opponent [`together`](crate::rally::together) plays against,
//! and every seat `--bots N` fills. It is client-local all the same: what
//! reaches the wire is the [`Move`] it returns, and nothing here is hashed.
//!
//! [`Opponent`] is that paddle as a [`Controller`], which is the form the
//! runtime takes; [`target`] and [`toward`] are the arithmetic under it, called
//! directly by `tests/bot.rs`.

use crate::table::{Court, Move, Play, Table};
use corvid::{Acting, Camera, Controller, I16F16, Updating};

/// How far from its target a paddle stops correcting, as a share of its own
/// half-height.
///
/// Not zero, and the reason is oscillation: a paddle that corrects every last
/// fraction of a millimetre alternates up and down for ever around the target,
/// which reads as a twitch and — because prediction repeats a seat's newest
/// action — mispredicts on every single tick of a networked session. A third
/// of the paddle is inside the part that returns the ball, so stopping there
/// costs nothing.
const SETTLED: f64 = 0.34;

/// How far towards the paddle's edge a shot is aimed, as a share of its
/// half-height.
///
/// The edge would be the strongest shot and the least forgiving: a paddle that
/// aims at its own corner misses whenever its prediction is a centimetre out.
/// Two thirds is most of the angle for a fraction of the risk.
const AIM: f64 = 0.66;

/// An opponent that is actually trying.
///
/// A pure function of the state and the court, so a peer in the netcode lab, the
/// opponent [`together`](crate::rally::together) plays against and a seat filled
/// by `--bots` all play the same paddle. What reaches the wire is the [`Move`]
/// it returns; nothing here is hashed.
///
/// # Where it reads the court from
///
/// [`court`](crate::court) and [`rules`](crate::rules), because an
/// [`Acting`] carries the state and not the level and [`Table`] does not carry
/// its own court. That is exact rather than approximate for this game: the two
/// constants are what [`opening`](crate::opening) puts in the session, and they
/// are also what [`Level::load`](corvid::Level::load) answers for the only
/// level this game has — so a run that opened on `--level` plays them too, and
/// there is no court a session could be on that this would mispredict.
///
/// A game with several courts would have to be handed the level instead, and
/// the honest place for that is the state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Opponent;

impl Controller<Table> for Opponent {
    /// Nothing to set. Which seat it plays is the runtime's to say, once per
    /// call, and how it plays is the two constants above.
    type Config = ();

    /// No device, which is what makes a seat filled by `--bots` cost a run
    /// nothing but the arithmetic.
    const REAL: bool = false;

    /// And no actions, because a controller that reads no device has nothing to
    /// declare. The declaration a run is sized from is the *player's*
    /// controller's, which is [`Hands`](crate::Hands).
    const SETS: &'static [corvid::SetDescriptor] = &[];

    fn new((): ()) -> Self {
        Self
    }

    fn configure(&mut self, (): ()) {}

    /// Where the ball is going to be, and which part of the paddle to meet it
    /// with.
    ///
    /// [`Acting::seat`] is which paddle, so one instance answers for however
    /// many seats a run gave it.
    fn action(&self, acting: Acting<'_, Table>) -> Move {
        let seat = usize::from(acting.seat.0);
        // A seat this game has no paddle for, which a two-seat roster cannot
        // produce and which is answered anyway: standing still is the one reply
        // that cannot move something that is not there. Aiming from a paddle
        // assumed to be at the middle would answer `Up` or `Down` for a seat
        // whose position is not a fact.
        let Some(paddle) = acting.state.paddles.get(seat) else {
            return Move::Still;
        };
        let court = crate::court();
        let rules = crate::rules();
        toward(
            paddle.at,
            target(seat, acting.state, &court, &rules),
            &court,
        )
    }

    /// Nothing accumulates: there is no camera to smooth and no cursor to cast.
    fn update(&mut self, _updating: Updating<'_, Table>) {}

    fn look(&self) -> Camera {
        Camera::default()
    }
}

/// Where this seat's paddle should be, in court metres.
///
/// The whole of the opponent, and it is worth reading as three cases rather
/// than as an algorithm: the ball is coming here, the ball is going away, or
/// there is no ball yet.
#[must_use]
pub fn target(seat: usize, table: &Table, court: &Court, rules: &Play) -> I16F16 {
    // Nothing to read yet. The middle is where a serve can be reached from
    // either direction, which is what a player who is not guessing does.
    if table.serve > 0 {
        return I16F16::ZERO;
    }

    let face = court.face(seat);
    let towards = if seat == 0 {
        table.ball.velocity.x().is_negative()
    } else {
        table.ball.velocity.x().is_positive()
    };
    if !towards {
        // Going away. Drift back towards the middle rather than staying where
        // the last rally left this paddle — the next shot can come to either
        // half, and the middle is the shortest worst case.
        return I16F16::ZERO;
    }

    let arrival = arrival(table, court, face);
    // And which part of the paddle to meet it with. The ball leaves towards the
    // side the contact was on, so hitting it below centre sends it downwards —
    // and the side to send it to is the one the opponent is furthest from.
    let opponent = table
        .paddles
        .get(1 - seat)
        .map_or(I16F16::ZERO, |paddle| paddle.at);
    let away = if opponent.is_negative() {
        I16F16::from_f64(AIM)
    } else {
        -I16F16::from_f64(AIM)
    };
    // Meeting the ball with the part of the paddle *nearer* the side it should
    // leave towards means putting the paddle's centre on the other side of it.
    let aimed = arrival - away * court.paddle.y();
    let reach = court.reach();
    // A shot aimed past what the paddle can reach is a shot not worth aiming:
    // getting there matters more than the angle.
    let _ = rules;
    aimed.clamp(-reach, reach)
}

/// Which direction to move, given where the paddle is and where it should be.
#[must_use]
pub fn toward(at: I16F16, target: I16F16, court: &Court) -> Move {
    let settled = court.paddle.y() * I16F16::from_f64(SETTLED);
    let away = target - at;
    if away > settled {
        Move::Up
    } else if away < -settled {
        Move::Down
    } else {
        Move::Still
    }
}

/// Where the ball will cross `face`, with every wall it bounces off on the way
/// folded in.
///
/// The ball travels in a straight line between bounces and the walls are
/// parallel, so this is one division and one fold rather than a simulation:
/// work out how many ticks until it reaches the paddle's plane, run the
/// vertical component out that far, and reflect the answer back into the court.
///
/// **A fold rather than a loop of bounces**, because the two are the same
/// answer and only one of them is bounded: a ball hit hard at a shallow angle
/// crosses the court in a few ticks and bounces twice, and a ball hit softly at
/// a steep one bounces a dozen times — and a loop would be a loop over a number
/// another player chose.
fn arrival(table: &Table, court: &Court, face: I16F16) -> I16F16 {
    let across = table.ball.velocity.x();
    if across.is_zero() {
        return table.ball.at.y();
    }
    let ticks = (face - table.ball.at.x()).saturating_div(across);
    let free = table
        .ball
        .at
        .y()
        .saturating_add(table.ball.velocity.y().saturating_mul(ticks));
    fold(free, court.half.y() - court.ball)
}

/// Reflects a height back into `-limit ..= limit`, as many times as it takes.
///
/// The court is a mirror box in one dimension: a ball that would have reached
/// `limit + d` is really at `limit - d`, and one that would have reached
/// `3·limit + d` has bounced twice and is at `-limit + d`. That is a fold about
/// a period of `2·limit`, which is one modulo and one comparison.
fn fold(height: I16F16, limit: I16F16) -> I16F16 {
    if limit <= I16F16::ZERO {
        return I16F16::ZERO;
    }
    let period = limit.saturating_mul(I16F16::from_f64(2.0));
    // Into `0 ..= 2·limit`, measuring from the bottom wall.
    let from_bottom = height.saturating_add(limit);
    let mut wrapped = from_bottom.saturating_rem(period.saturating_mul(I16F16::from_f64(2.0)));
    if wrapped.is_negative() {
        wrapped = wrapped.saturating_add(period.saturating_mul(I16F16::from_f64(2.0)));
    }
    // A full period is a round trip: the first half is the way up and the
    // second is the way back down.
    let bounced = if wrapped > period {
        period.saturating_mul(I16F16::from_f64(2.0)) - wrapped
    } else {
        wrapped
    };
    bounced - limit
}

#[cfg(test)]
mod tests {
    //! The prediction, on its own. Everything here is arithmetic with no
    //! session, no peer and no link in it.

    #![allow(
        clippy::panic,
        clippy::unwrap_used,
        reason = "a failed assertion in a test is a failed test, which is what a test is for"
    )]

    use super::{fold, toward};
    use crate::{Move, court};
    use corvid::I16F16;
    #[test]
    fn a_height_inside_the_court_is_where_it_was() {
        let limit = I16F16::from_f64(4.0);
        for height in [-3.5, -1.0, 0.0, 2.25, 3.9] {
            assert_eq!(
                fold(I16F16::from_f64(height), limit),
                I16F16::from_f64(height)
            );
        }
    }

    #[test]
    fn a_height_past_a_wall_comes_back_the_same_distance_inside_it() {
        let limit = I16F16::from_f64(4.0);
        // One bounce off the top, and one off the bottom.
        assert_eq!(fold(I16F16::from_f64(5.0), limit), I16F16::from_f64(3.0));
        assert_eq!(fold(I16F16::from_f64(-5.0), limit), I16F16::from_f64(-3.0));
        // And two, which is where a fold differs from a clamp *and* from one
        // reflection: thirteen metres of travel from the middle of an
        // eight-metre court goes up four, down eight, and up one — so it ends
        // one metre below the middle of the far half, on the *other* side from
        // where a single bounce would have left it. A clamp would have answered
        // the wall both times.
        assert_eq!(fold(I16F16::from_f64(13.0), limit), I16F16::from_f64(-3.0));
        assert_eq!(fold(I16F16::from_f64(-13.0), limit), I16F16::from_f64(3.0));
    }

    #[test]
    fn a_paddle_close_enough_stops_rather_than_twitching() {
        let court = court();
        let near = court.paddle.y() * I16F16::from_f64(0.1);
        assert_eq!(toward(I16F16::ZERO, near, &court), Move::Still);
        assert_eq!(toward(I16F16::ZERO, court.paddle.y(), &court), Move::Up);
        assert_eq!(toward(I16F16::ZERO, -court.paddle.y(), &court), Move::Down);
    }
}
