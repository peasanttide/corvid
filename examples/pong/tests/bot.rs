//! Whether the opponent is actually trying.
//!
//! A bot is easy to write and easy to write badly, and the two are hard to tell
//! apart by watching for a few seconds. What tells them apart is a long game
//! against itself: a paddle that follows the ball's current height loses points
//! to anything hit at an angle, and one that goes where the ball is *going*
//! rallies until somebody gets an angle it cannot reach.
//!
//! So this plays one out and counts.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "every test here returns a `Result` so that a failure reaches for `?` rather than unwrapping, and asserts as well — a failed assertion in a test is a failed test, which is what a test is for"
)]

use std::sync::Arc;

use corvid::FinePoint;
use corvid::I16F16;
use corvid::{Acting, Controller, Input, Time};
use corvid::{Player, PlayerId, Presence, State};
use pong::{
    Ball, Contact, Court, Move, Opponent, Paddle, Play, SEATS, Table, bot, court, origin, rules,
};

/// One tick, with both paddles played by whichever policy is given.
fn step(table: &Table, level: &Arc<Court>, rules: &Play, actions: [Move; SEATS]) -> Table {
    let players: Vec<Player<'_, Move>> = actions
        .iter()
        .enumerate()
        .map(|(seat, action)| Player {
            id: PlayerId(u16::try_from(seat).unwrap_or(0)),
            presence: Presence::Active,
            action,
        })
        .collect();
    table
        .clone()
        .tick(level, &players, rules, &mut corvid::Discard::new())
}

/// Plays `ticks` ticks with both seats driven by `play`, and answers the state
/// and how many times the ball was returned.
fn rally(ticks: u64, play: impl Fn(usize, &Table, &Court, &Play) -> Move) -> (Table, u32) {
    let level = Arc::new(court());
    let rules = rules();
    let mut table = origin();
    let mut returns = 0_u32;
    for _ in 0..ticks {
        let actions = [
            play(0, &table, &level, &rules),
            play(1, &table, &level, &rules),
        ];
        table = step(&table, &level, &rules, actions);
        if matches!(table.contact, Some(Contact::Paddle { .. })) {
            returns += 1;
        }
    }
    (table, returns)
}

/// The bot that only watches the ball's current height, which is what this
/// example shipped with first.
///
/// Kept here as the control. A test that says the opponent is good needs
/// something for "good" to be better *than*, and the honest comparison is
/// against the obvious implementation rather than against nothing.
fn follows_the_ball(seat: usize, table: &Table, court: &Court, _rules: &Play) -> Move {
    let Some(paddle) = table.paddles.get(seat) else {
        return Move::Still;
    };
    bot::toward(paddle.at, table.ball.at.y(), court)
}

/// **The claim.** Two of these rally for a long time and score rarely.
///
/// Two thousand ticks is a minute of play. A pair of paddles that could not
/// read a shot would concede several points a minute — the control below does —
/// and a pair that can turns the same minute into a rally with a handful of
/// points in it.
#[test]
fn two_bots_rally_rather_than_trading_goals() {
    let (table, returns) = rally(2_000, |seat, table, court, rules| {
        let Some(paddle) = table.paddles.get(seat) else {
            return Move::Still;
        };
        bot::toward(paddle.at, bot::target(seat, table, court, rules), court)
    });

    let goals = u32::from(table.scores[0]) + u32::from(table.scores[1]);
    assert!(
        returns > 40,
        "the paddles returned the ball {returns} times in two thousand ticks, \
         which is not a rally",
    );
    assert!(
        goals <= 6,
        "the paddles conceded {goals} goals in two thousand ticks, which is not \
         an opponent trying",
    );
}

/// And it is better than the obvious one, measured rather than asserted.
#[test]
fn predicting_where_the_ball_will_be_beats_following_where_it_is() {
    let (predicting, hits) = rally(2_000, |seat, table, court, rules| {
        let Some(paddle) = table.paddles.get(seat) else {
            return Move::Still;
        };
        bot::toward(paddle.at, bot::target(seat, table, court, rules), court)
    });
    let (following, misses) = rally(2_000, follows_the_ball);

    let predicted_goals = u32::from(predicting.scores[0]) + u32::from(predicting.scores[1]);
    let followed_goals = u32::from(following.scores[0]) + u32::from(following.scores[1]);
    assert!(
        predicted_goals < followed_goals,
        "predicting conceded {predicted_goals} goals and following conceded \
         {followed_goals}; the prediction is not buying anything",
    );
    assert!(
        hits > misses,
        "predicting returned the ball {hits} times and following {misses}",
    );
}

/// The controller a `--bots N` seat is filled with plays the seat it is asked
/// for.
///
/// The tests above are about whether the arithmetic is any good; this one is
/// about the wiring, and it is the half a rally cannot see. [`Opponent`] holds
/// no seat of its own — one instance answers for every seat a run gave it — so
/// the only thing deciding which paddle it moves is
/// [`Acting::seat`](corvid::Acting), and a bot that read the wrong one would
/// play a perfectly good paddle at the wrong end of the court while the ball
/// went past the one it was supposed to be defending.
#[test]
fn the_opponent_moves_the_paddle_of_the_seat_it_is_asked_for() {
    let level = court();
    let rules = rules();
    let reach = level.reach();

    // The ball in the middle, on its way to seat zero's end. Seat zero's paddle
    // is at the top of the court and has to come down to meet it; seat one's is
    // already in the middle, which is where a paddle the ball is travelling away
    // from drifts back to.
    let table = Table {
        ball: Ball {
            at: FinePoint::new(I16F16::ZERO, I16F16::ZERO, I16F16::ZERO),
            velocity: FinePoint::new(-rules.serve_speed, I16F16::ZERO, I16F16::ZERO),
        },
        paddles: [Paddle { at: reach }, Paddle { at: I16F16::ZERO }],
        serve: 0,
        ..origin()
    };
    let asked = |seat: usize| {
        Opponent.action(Acting {
            state: &table,
            input: &Input::new(pong::action::SETS),
            time: Time::default(),
            seat: PlayerId(u16::try_from(seat).unwrap()),
        })
    };

    // Each answer is the arithmetic applied to *that* seat's own paddle, which
    // is the whole claim.
    for seat in 0..SEATS {
        assert_eq!(
            asked(seat),
            bot::toward(
                table.paddles[seat].at,
                bot::target(seat, &table, &level, &rules),
                &level,
            ),
            "the opponent did not play seat {seat}'s own paddle",
        );
    }

    // And the two seats really are being told apart: one of them has somewhere
    // to be and the other is already there, so a bot that ignored the seat it
    // was handed would answer one of these wrongly whichever seat it assumed.
    assert_eq!(asked(0), Move::Down);
    assert_eq!(asked(1), Move::Still);
}

/// A bot reaches a shot aimed at the corner.
///
/// The specific thing prediction buys: a ball on its way to the top of the
/// court while the paddle sits at the bottom. Following the ball's height gets
/// there eventually and eventually is after it has gone past.
#[test]
fn a_corner_shot_is_reached() {
    let level = Arc::new(court());
    let rules = rules();
    let reach = level.reach();

    // A ball crossing the court from seat one's end towards the top of seat
    // zero's, with seat zero's paddle at the bottom.
    let mut table = Table {
        ball: Ball {
            at: FinePoint::new(level.half.x() * I16F16::from_f64(0.8), -reach, I16F16::ZERO),
            velocity: FinePoint::new(-rules.serve_speed, I16F16::from_f64(0.12), I16F16::ZERO),
        },
        paddles: [
            pong::Paddle { at: -reach },
            pong::Paddle { at: I16F16::ZERO },
        ],
        serve: 0,
        ..origin()
    };

    let mut played = false;
    for _ in 0..120 {
        let action = {
            let Some(paddle) = table.paddles.first() else {
                break;
            };
            bot::toward(paddle.at, bot::target(0, &table, &level, &rules), &level)
        };
        table = step(&table, &level, &rules, [action, Move::Still]);
        if matches!(table.contact, Some(Contact::Paddle { seat: 0, .. })) {
            played = true;
            break;
        }
        if table.scores[1] > 0 {
            break;
        }
    }

    assert!(
        played,
        "the bot did not reach a shot aimed away from where its paddle started",
    );
}
