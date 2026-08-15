//! The simulation, on its own: no peers, no link, no runtime.
//!
//! Everything the netcode tests assert rests on `tick` being a pure function of
//! what it is handed, so this is where that is checked directly -- and where the
//! game is checked to be pong rather than a ball that falls out of the court.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "every test here returns a `Result` so that a failure reaches for `?` rather than unwrapping, and asserts as well -- a failed assertion in a test is a failed test, which is what a test is for"
)]

use std::sync::Arc;

use corvid::FinePoint;
use corvid::I16F16;
use corvid::digest;
use corvid::{PlayerId, PlayerState, Presence, State};
use pong::{Ball, Contact, Court, Move, Play, SEATS, Table, court, origin, rules};

/// One tick, with both seats doing what they are told.
fn step(table: &Table, level: &Arc<Court>, rules: &Play, actions: [Move; SEATS]) -> Table {
    let players: Vec<PlayerState<Move>> = actions
        .into_iter()
        .enumerate()
        .map(|(seat, action)| PlayerState {
            id: PlayerId(u16::try_from(seat).unwrap_or(u16::MAX)),
            presence: Presence::Active,
            action,
        })
        .collect();
    table
        .clone()
        .tick(level, &players, rules, &mut corvid::Discard::new())
}

/// Plays `ticks` ticks with both seats still.
fn drift(from: Table, ticks: u64) -> Table {
    let level = Arc::new(court());
    let rules = rules();
    let mut table = from;
    for _ in 0..ticks {
        table = step(&table, &level, &rules, [Move::Still; SEATS]);
    }
    table
}

/// A table with the ball in play, travelling as given.
fn served(at: FinePoint, velocity: FinePoint) -> Table {
    Table {
        ball: Ball { at, velocity },
        serve: 0,
        ..origin()
    }
}

#[test]
fn a_paddle_moves_and_stops_at_the_wall() {
    let level = Arc::new(court());
    let rules = rules();
    let mut table = origin();
    for _ in 0..500 {
        table = step(&table, &level, &rules, [Move::Up, Move::Down]);
    }

    let reach = level.reach();
    assert_eq!(table.paddles[0].at, reach);
    assert_eq!(table.paddles[1].at, -reach);
}

#[test]
fn holding_both_directions_stands_still() {
    // Not a rule of the simulation -- the simulation is handed one `Move` -- but
    // of the client that builds one, so this is where the two meet: there is no
    // action that means "up and down", which is what makes the tie impossible
    // to get wrong on the wire.
    let level = Arc::new(court());
    let table = step(&origin(), &level, &rules(), [Move::Still; SEATS]);
    assert_eq!(table.paddles, origin().paddles);
}

#[test]
fn the_ball_is_served_after_the_countdown() {
    let opening = origin();
    assert!(opening.serve > 0, "a session opens waiting to serve");
    assert_eq!(opening.ball.velocity, FinePoint::default());

    let waiting = drift(opening.clone(), u64::from(opening.serve) - 1);
    assert_eq!(waiting.serve, 1);
    assert_eq!(waiting.ball.velocity, FinePoint::default());

    let served = drift(opening, u64::from(origin().serve));
    assert_eq!(served.serve, 0);
    assert!(
        !served.ball.velocity.x().is_zero(),
        "the serve left the centre with no horizontal speed",
    );
}

#[test]
fn the_ball_bounces_off_the_top_and_the_bottom() {
    let level = court();
    let rules = rules();
    let high = level.half.y() - level.ball - I16F16::from_f64(0.05);
    let table = served(
        FinePoint::new(I16F16::ZERO, high, I16F16::ZERO),
        FinePoint::new(I16F16::from_f64(0.1), I16F16::from_f64(0.2), I16F16::ZERO),
    );

    let next = step(&table, &Arc::new(level), &rules, [Move::Still; SEATS]);
    assert!(
        next.ball.velocity.y().is_negative(),
        "the ball went through the top of the court",
    );
    assert!(matches!(next.contact, Some(Contact::Wall { .. })));
}

#[test]
fn a_paddle_returns_the_ball_and_a_miss_scores() {
    let level = court();
    let rules = rules();
    let face = level.face(0);

    // Straight at the middle of seat zero's paddle, which is where it starts.
    let onto = served(
        FinePoint::new(face + I16F16::from_f64(0.1), I16F16::ZERO, I16F16::ZERO),
        FinePoint::new(I16F16::from_f64(-0.2), I16F16::ZERO, I16F16::ZERO),
    );
    let hit = step(
        &onto,
        &Arc::new(level.clone()),
        &rules,
        [Move::Still; SEATS],
    );
    assert!(
        matches!(hit.contact, Some(Contact::Paddle { seat: 0, .. })),
        "the paddle did not play a ball aimed at its middle: {:?}",
        hit.contact,
    );
    assert!(
        hit.ball.velocity.x().is_positive(),
        "the ball did not come back"
    );
    assert_eq!(hit.scores, [0, 0]);

    // And the same ball with the paddle out of the way.
    let past = Table {
        paddles: [pong::Paddle { at: level.reach() }, pong::Paddle::default()],
        ..onto
    };
    let missed = step(&past, &Arc::new(level), &rules, [Move::Still; SEATS]);
    let missed = drift(missed, 12);
    assert_eq!(missed.scores, [0, 1], "seat one did not score the point");
    assert!(missed.serve > 0, "no serve was set up after the goal");
}

#[test]
fn a_ball_hit_off_centre_leaves_at_an_angle() {
    let level = court();
    let rules = rules();
    let face = level.face(1);
    let offset = level.paddle.y() - I16F16::from_f64(0.1);

    let onto = Table {
        ball: Ball {
            at: FinePoint::new(face - I16F16::from_f64(0.1), offset, I16F16::ZERO),
            velocity: FinePoint::new(I16F16::from_f64(0.2), I16F16::ZERO, I16F16::ZERO),
        },
        serve: 0,
        ..origin()
    };
    let hit = step(&onto, &Arc::new(level), &rules, [Move::Still; SEATS]);
    assert!(
        matches!(hit.contact, Some(Contact::Paddle { seat: 1, .. })),
        "the ball was not played: {:?}",
        hit.contact,
    );
    assert!(
        hit.ball.velocity.y().is_positive(),
        "a ball hit above the paddle's centre did not leave upwards",
    );
}

#[test]
fn a_fast_ball_cannot_pass_through_a_paddle() {
    // The whole reason the paddle test is a crossing rather than an overlap. At
    // the top speed the ball travels several times the paddle's thickness in
    // one tick, so a check that asked "is the ball inside the paddle now" would
    // miss every fast shot -- and a pong where hard shots go through the paddle
    // is a pong nobody can play.
    let level = court();
    let rules = rules();
    let face = level.face(0);
    let onto = served(
        FinePoint::new(
            face + rules.top_speed - I16F16::from_f64(0.01),
            I16F16::ZERO,
            I16F16::ZERO,
        ),
        FinePoint::new(-rules.top_speed, I16F16::ZERO, I16F16::ZERO),
    );
    let hit = step(&onto, &Arc::new(level), &rules, [Move::Still; SEATS]);
    assert!(
        matches!(hit.contact, Some(Contact::Paddle { seat: 0, .. })),
        "a ball at top speed went through the paddle: {:?}",
        hit.ball,
    );
}

#[test]
fn a_game_ends_at_the_target_and_then_stands_still() {
    let rules = rules();
    let level = Arc::new(court());
    let nearly = Table {
        scores: [rules.target - 1, 0],
        serve: 0,
        ball: Ball {
            at: FinePoint::new(level.half.x(), I16F16::ZERO, I16F16::ZERO),
            velocity: FinePoint::new(I16F16::from_f64(0.2), I16F16::ZERO, I16F16::ZERO),
        },
        ..origin()
    };
    let won = step(&nearly, &level, &rules, [Move::Still; SEATS]);
    assert_eq!(won.over, Some(0));
    assert_eq!(won.scores[0], rules.target);

    // And nothing moves afterwards, however hard the players push.
    let after = step(&won, &level, &rules, [Move::Up, Move::Down]);
    assert_eq!(after.paddles, won.paddles);
    assert_eq!(after.ball, won.ball);
}

#[test]
fn a_tick_is_the_same_function_whatever_the_scratch_was() {
    // `Scratch` is `()` here, so this is trivially true -- and it is asserted
    // anyway, because the obligation is on the game rather than on the type and
    // a later version of this example that grows a cache should fail this test
    // the moment the cache is read rather than written.
    let level = Arc::new(court());
    let rules = rules();
    let table = drift(origin(), 40);

    let with = step(&table, &level, &rules, [Move::Up, Move::Down]);
    let without = step(&table, &level, &rules, [Move::Up, Move::Down]);
    assert_eq!(digest(&with), digest(&without));
}

/// What a client draws at either end of the interpolation is the state it was
/// extracted from, exactly.
///
/// The obligation `Render::draw` is held to, checked at the two ends where it
/// matters: at zero the picture is `previous` and at one it is `current`, bit
/// for bit rather than nearly. It is checked here rather than in a picture,
/// because a PNG is compared with a tolerance and a tolerance is what would
/// hide this.
#[cfg(feature = "render")]
#[test]
fn the_drawn_ball_is_exact_at_both_ends() {
    use corvid::Factor16;
    let level = Arc::new(court());
    let before = served(
        FinePoint::new(I16F16::from_f64(-1.5), I16F16::from_f64(0.25), I16F16::ZERO),
        FinePoint::new(I16F16::from_f64(0.2), I16F16::from_f64(0.1), I16F16::ZERO),
    );
    let after = step(&before, &level, &rules(), [Move::Still; SEATS]);

    assert_eq!(
        pong::ball_at(&before, &after, Factor16::ZERO),
        [before.ball.at.x(), before.ball.at.y()],
    );
    assert_eq!(
        pong::ball_at(&before, &after, Factor16::ONE),
        [after.ball.at.x(), after.ball.at.y()],
    );
}

#[test]
fn a_hundred_ticks_are_the_same_hundred_ticks_every_time() {
    // The claim the whole design rests on, stated at its smallest: the same
    // opening and the same actions produce the same digest. Two peers doing
    // this on two machines is what `tests/session.rs` measures.
    let play = || {
        let level = Arc::new(court());
        let rules = rules();
        let mut table = origin();
        for at in 0..100_u64 {
            let mine = if at % 7 < 3 { Move::Up } else { Move::Down };
            let theirs = if at % 5 < 2 { Move::Down } else { Move::Still };
            table = step(&table, &level, &rules, [mine, theirs]);
        }
        digest(&table)
    };
    assert_eq!(play(), play());
}

/// The plane the ball bounces off is the drawn paddle's court-facing edge.
///
/// `Court::face` is the bounce plane and `Court::centre` is what the client
/// draws the rectangle around, and the two are only consistent while they
/// differ by exactly half a paddle. Making them the same number -- centring the
/// sprite on `face` -- puts the drawn edge at `paddle.x() + ball`, so the ball
/// reaches it before the plane and buries itself in the paddle on every
/// return.
///
/// Nothing about this is visible to a digest -- both peers computed the same
/// wrong-looking bounce -- which is exactly why it wants an assertion.
#[test]
fn the_bounce_plane_is_the_drawn_paddles_near_edge() {
    let level = court();
    for seat in 0..SEATS {
        let face = level.face(seat);
        let centre = level.centre(seat);
        let half = level.paddle.x();

        // The near edge of the rectangle spanning `centre +/- half`, on the side
        // the middle of the court is.
        let near = if seat == 0 {
            centre + half
        } else {
            centre - half
        };
        assert_eq!(
            near, face,
            "seat {seat} draws a paddle whose near edge is {near:?} and bounces \
             the ball off {face:?}",
        );

        // And the whole rectangle is outside the face rather than straddling
        // it, which is the same statement from the other end.
        let far = if seat == 0 {
            centre - half
        } else {
            centre + half
        };
        assert!(
            far.abs() > face.abs(),
            "seat {seat}'s paddle reaches {far:?}, which is nearer the middle \
             than its own face at {face:?}",
        );
    }
}
