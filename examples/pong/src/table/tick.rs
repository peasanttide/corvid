//! The four steps one tick is made of.
//!
//! The seam against `mod.rs` is that nothing here is a type: these are the
//! moves, the serve and the two collisions, in the order
//! [`Table::tick`](super::Table::tick) applies them, and each of them writes
//! the state in place.

use corvid::{FinePoint, I16F16, PlayerId, PlayerState};

use crate::table::{Ball, Contact, Court, Move, Play, SEATS, Table};

/// Moves each seated player's paddle, and clamps it to the court.
pub(super) fn move_paddles(
    table: &mut Table,
    level: &Court,
    rules: &Play,
    players: &[PlayerState<Move>],
) {
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
pub(super) fn serve(table: &mut Table, rules: &Play) {
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
pub(super) fn bounce_off_walls(table: &mut Table, level: &Court) {
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
pub(super) fn play_off_paddles(table: &mut Table, level: &Court, rules: &Play, was: FinePoint) {
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
pub(super) fn score(table: &mut Table, level: &Court, rules: &Play) {
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
