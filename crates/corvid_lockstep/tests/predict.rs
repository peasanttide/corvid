//! The confirmation frontier, the datagram it is read out of, and what a real
//! action does to a prediction.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Action, push};
use corvid_behavior::PlayerId;
use corvid_hash::Digest;
use corvid_lockstep::{Correction, Datagram, Frontier, WINDOW, absorb, action_at, predict, row_at};
use corvid_replay::ActionLog;
use corvid_time::Tick;
/// A log of `seats` seats with room out to `rows`, and a frontier to match.
fn log(seats: u16, rows: u64) -> (ActionLog<Action>, Frontier) {
    let mut log = ActionLog::new(Tick::ZERO, seats);
    log.extend_to(Tick(rows)).unwrap();
    (log, Frontier::new(seats))
}

/// Confirms one cell in both places at once, which is what
/// [`absorb`] does and what a hand-built fixture has to do too.
fn confirm(
    log: &mut ActionLog<Action>,
    frontier: &mut Frontier,
    at: u64,
    seat: u16,
    action: Action,
) {
    log.set(Tick(at), PlayerId(seat), action).unwrap();
    frontier.observe(PlayerId(seat), Tick(at));
}

/// A datagram from `seat` for `head`, carrying `actions` oldest first.
fn sent(seat: u16, head: u64, actions: [Action; WINDOW]) -> Datagram<Action> {
    Datagram {
        seat: PlayerId(seat),
        first: Tick(head.saturating_sub(WINDOW as u64 - 1)),
        actions: actions.to_vec(),
        heard: None,
        marked: Tick::ZERO,
        mark: Digest::ZERO,
    }
}

#[test]
fn a_datagram_at_the_opening_carries_the_opening_and_nothing_before_it() {
    let (mut log, _) = log(1, 4);
    log.set(Tick::ZERO, PlayerId(0), push(9)).unwrap();

    let sent = Datagram::build(
        &log,
        PlayerId(0),
        Tick::ZERO,
        None,
        None,
        Tick::ZERO,
        Digest::ZERO,
    );

    // One row, because there is one tick. The window reaches () `WINDOW` rows
    // and is clamped to what the log holds, so a datagram sent on a session's
    // first tick does not pad three ticks that never ran — it used to, and the
    // padding was three rows of `Idle` naming the opening tick over and over.
    assert_eq!(sent.actions, [push(9)]);
    assert_eq!(sent.first, Tick::ZERO);
    assert_eq!(sent.head(), Tick::ZERO);
    assert_eq!(
        sent.ticks().map(|(tick, _)| tick).collect::<Vec<_>>(),
        [Tick::ZERO],
    );
}

#[test]
fn a_datagram_carries_everything_the_far_end_has_not_acknowledged() {
    let (mut log, _) = log(1, 40);
    for at in 0..30_i16 {
        log.set(Tick(u64::try_from(at).unwrap_or(0)), PlayerId(0), push(at))
            .unwrap();
    }

    // A far end that is up to date gets the minimum window.
    let fresh = Datagram::build(
        &log,
        PlayerId(0),
        Tick(29),
        Some(Tick(28)),
        Some(Tick(28)),
        Tick(28),
        Digest::ZERO,
    );
    assert_eq!(fresh.rows(), WINDOW);

    // One that has heard nothing since tick five gets everything since tick
    // five, which is what makes a link that went away for a while recoverable.
    let behind = Datagram::build(
        &log,
        PlayerId(0),
        Tick(29),
        Some(Tick(5)),
        Some(Tick(5)),
        Tick(5),
        Digest::ZERO,
    );
    assert_eq!(behind.first, Tick(6));
    assert_eq!(behind.head(), Tick(29));
    assert_eq!(behind.rows(), 24);
}

#[test]
fn a_datagram_round_trips_through_the_wire() {
    let sent = sent(3, 41, [push(1), Action::Idle, Action::Build, push(-7)]);
    let bytes = corvid_wire::encode(&sent).unwrap();
    let read: Datagram<Action> = corvid_wire::decode(&bytes).unwrap();
    assert_eq!(read, sent);
    assert_eq!(corvid_wire::encode(&read).unwrap(), bytes);
}

#[test]
fn agreed_is_the_seat_that_is_furthest_behind() {
    let mut frontier = Frontier::new(3);
    frontier.observe(PlayerId(0), Tick(10));
    frontier.observe(PlayerId(1), Tick(12));
    frontier.observe(PlayerId(2), Tick(9));
    assert_eq!(frontier.agreed(), Tick(9));
}

#[test]
fn observe_never_moves_a_seat_backwards() {
    let mut frontier = Frontier::new(1);
    frontier.observe(PlayerId(0), Tick(12));
    frontier.observe(PlayerId(0), Tick(4));
    assert_eq!(frontier.of(PlayerId(0)), Tick(12));
    assert_eq!(frontier.agreed(), Tick(12));
}

#[test]
fn predicted_at_a_tick_every_seat_has_confirmed_is_empty() {
    let mut frontier = Frontier::new(2);
    frontier.observe(PlayerId(0), Tick(5));
    frontier.observe(PlayerId(1), Tick(6));
    assert_eq!(frontier.predicted(Tick(5)).collect::<Vec<_>>(), []);

    // And the seat that has only reached five is the one predicted at six.
    assert_eq!(
        frontier.predicted(Tick(6)).collect::<Vec<_>>(),
        [PlayerId(0)]
    );
}

#[test]
fn the_last_action_repeats_into_the_absent_ticks() {
    let (mut log, mut frontier) = log(1, 12);
    confirm(&mut log, &mut frontier, 5, 0, push(4));

    for at in [6, 7, 8] {
        assert_eq!(
            action_at(&log, &frontier, Tick(at), PlayerId(0)),
            Some(&push(4)),
            "tick {at} repeats the action from tick 5",
        );
    }

    let mut row = Vec::new();
    row_at(&log, &frontier, Tick(8), &mut row);
    assert_eq!(row, [push(4)]);
}

#[test]
fn a_seat_that_has_never_acted_predicts_the_default() {
    let (mut log, mut confirmed) = log(2, 12);
    confirm(&mut log, &mut confirmed, 3, 0, Action::Build);

    assert_eq!(action_at(&log, &confirmed, Tick(4), PlayerId(1)), None);

    let predicted = predict(&mut log, &confirmed, Tick(4)).unwrap();
    assert_eq!(predicted.seats, 2, "neither seat has confirmed tick 4");
    assert_eq!(
        predicted.from_default, 1,
        "and one of the two has nothing to repeat",
    );

    let mut row = Vec::new();
    row_at(&log, &confirmed, Tick(4), &mut row);
    assert_eq!(row, [Action::Build, Action::Idle]);
}

#[test]
fn predict_grows_the_log_to_the_row_it_is_asked_about() {
    let (mut log, frontier) = log(1, 2);
    assert_eq!(log.last(), Tick(3));
    predict(&mut log, &frontier, Tick(9)).unwrap();
    assert_eq!(log.last(), Tick(10));
}

#[test]
fn an_action_equal_to_the_prediction_is_agreed() {
    let (mut log, mut frontier) = log(1, 12);
    confirm(&mut log, &mut frontier, 3, 0, push(4));

    let rows = |log: &ActionLog<Action>, frontier: &Frontier| {
        let mut every = Vec::new();
        for at in 4..=6 {
            let mut row = Vec::new();
            row_at(log, frontier, Tick(at), &mut row);
            every.push(row);
        }
        every
    };
    let before = rows(&log, &frontier);

    // Ticks 4, 5 and 6 were all predicted as a repeat of tick 3, and that is
    // what arrives.
    let answer = absorb(&mut log, &mut frontier, &sent(0, 6, [push(4); WINDOW])).unwrap();

    assert_eq!(answer, Correction::Agreed);
    assert_eq!(
        rows(&log, &frontier),
        before,
        "the rows those ticks simulate against are what they already were, \
         which is why nothing is re-simulated",
    );
    assert_eq!(frontier.of(PlayerId(0)), Tick(6));
}

/// A prediction is not written down, so confirming one *does* move the log's
/// entries — from `Action::default()`, which is what an unconfirmed entry holds,
/// to the action that arrived.
///
/// [`ActionLog`] has one writer and it confirms what it writes, so a prediction
/// stored there would make the next real action a
/// [`Refused::Confirmed`](corvid_lockstep::Refused) rather than a correction.
/// The prediction is therefore read out of the log and the frontier every time
/// it is needed, and this is the consequence: an agreeing action still bumps the
/// log's generation, which costs the snapshots after it and costs nothing else.
#[test]
fn confirming_a_prediction_moves_the_log_even_when_it_agreed() {
    let (mut log, mut frontier) = log(1, 12);
    confirm(&mut log, &mut frontier, 3, 0, push(4));

    assert_eq!(log.get(Tick(4), PlayerId(0)), Some(&Action::Idle));
    assert!(!log.is_confirmed(Tick(4), PlayerId(0)));
    let generation = log.generation_at(Tick(6));

    absorb(&mut log, &mut frontier, &sent(0, 6, [push(4); WINDOW])).unwrap();

    assert_eq!(log.get(Tick(4), PlayerId(0)), Some(&push(4)));
    assert!(log.generation_at(Tick(6)) > generation);
}

#[test]
fn an_action_different_from_the_prediction_names_its_own_tick() {
    let (mut log, mut frontier) = log(1, 12);
    confirm(&mut log, &mut frontier, 3, 0, push(4));

    let answer = absorb(
        &mut log,
        &mut frontier,
        &sent(0, 6, [push(4), push(4), Action::Idle, Action::Idle]),
    )
    .unwrap();

    // Tick 5 is where the repeat stopped being right. Not tick 6, which the
    // corrected tick 5 then predicted correctly, and not tick 6 because the
    // stale ticks start there.
    assert_eq!(answer, Correction::Mispredicted { at: Tick(5) });
}

#[test]
fn a_duplicate_datagram_changes_nothing() {
    let (mut log, mut frontier) = log(1, 12);
    let arrived = sent(0, 6, [push(1), push(2), push(3), push(4)]);

    let first = absorb(&mut log, &mut frontier, &arrived).unwrap();
    assert_eq!(first, Correction::Mispredicted { at: Tick(3) });
    let settled = corvid_wire::encode(&log).unwrap();

    for again in 0..4 {
        let answer = absorb(&mut log, &mut frontier, &arrived).unwrap();
        assert_eq!(answer, Correction::Duplicate, "arrival {again}");
        assert_eq!(corvid_wire::encode(&log).unwrap(), settled);
    }
}

#[test]
fn a_contradicting_datagram_is_a_contradiction() {
    let (mut log, mut frontier) = log(1, 12);
    absorb(
        &mut log,
        &mut frontier,
        &sent(0, 6, [push(1), push(2), push(3), push(4)]),
    )
    .unwrap();

    let answer = absorb(
        &mut log,
        &mut frontier,
        &sent(0, 6, [push(1), push(2), push(9), push(4)]),
    )
    .unwrap();

    assert_eq!(answer, Correction::Contradiction { at: Tick(5) });
}

#[test]
fn a_datagram_far_in_the_future_is_ignored() {
    let (mut log, mut frontier) = log(1, 12);

    let answer = absorb(
        &mut log,
        &mut frontier,
        &sent(0, 1_000_000, [Action::Idle; WINDOW]),
    );

    // Nothing is recorded and nothing grows: a tick number arrives from
    // somewhere else, and a log that grew to whatever it said would be a
    // request for as much memory as the number.
    //
    // Ignored rather than refused, and the difference matters where it is
    // called from: a peer that stopped on this would be a peer any stranger
    // with a socket could stop.
    assert_eq!(answer, Ok(Correction::Duplicate), "{answer:?}");
    assert_eq!(log.last(), Tick(13), "and the log is where it was");
    assert!(!frontier.acted(PlayerId(0)), "and nothing was confirmed");
}

#[test]
fn a_datagram_older_than_the_log_says_nothing() {
    let mut log = ActionLog::<Action>::new(Tick(100), 1);
    log.extend_to(Tick(110)).unwrap();
    let mut frontier = Frontier::new(1);

    let answer = absorb(&mut log, &mut frontier, &sent(0, 3, [push(1); WINDOW])).unwrap();
    assert_eq!(answer, Correction::Duplicate);
}
