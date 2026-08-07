//! The dense action log: where an entry lives, what an absent one is, and the
//! one refusal that makes a log authoritative.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_behavior::PlayerId;

use corvid_replay::{ActionLog, Refused};

use corvid_time::Tick;
/// A small action, independent of the counter game, so that these tests are
/// about the log rather than about a simulation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Action {
    #[default]
    Idle,
    Bump,
    Reset,
}

/// A three-seat log covering ticks 10 through 13.
fn log() -> ActionLog<Action> {
    let mut log = ActionLog::new(Tick(10), 3);
    log.extend_to(Tick(13)).unwrap();
    log
}

#[test]
fn an_entry_is_at_tick_times_players_plus_player() {
    let mut log = log();
    log.set(Tick(12), PlayerId(2), Action::Bump).unwrap();

    // The row is the whole point: if the index were `player * ticks + tick`,
    // or `(tick - first)` off by one, or the seat added before the row was
    // scaled, this action would land on some other row and every one of the
    // four assertions below would move.
    assert_eq!(
        log.row(Tick(12)),
        [Action::Idle, Action::Idle, Action::Bump]
    );
    assert_eq!(log.row(Tick(11)), [Action::Idle; 3]);
    assert_eq!(log.row(Tick(13)), [Action::Idle; 3]);
    assert_eq!(log.get(Tick(12), PlayerId(2)), Some(&Action::Bump));
}

#[test]
fn a_seat_and_a_tick_are_not_interchangeable() {
    let mut log = ActionLog::new(Tick::ZERO, 4);
    log.extend_to(Tick(3)).unwrap();
    // Seat 1 at tick 2 and seat 2 at tick 1 differ only in which factor is
    // scaled by the row width. A log that multiplied by the seat instead would
    // put both at the same index and this would be one action, not two.
    log.set(Tick(2), PlayerId(1), Action::Bump).unwrap();
    log.set(Tick(1), PlayerId(2), Action::Reset).unwrap();

    assert_eq!(log.get(Tick(2), PlayerId(1)), Some(&Action::Bump));
    assert_eq!(log.get(Tick(1), PlayerId(2)), Some(&Action::Reset));
    assert_eq!(log.get(Tick(1), PlayerId(1)), Some(&Action::Idle));
    assert_eq!(log.get(Tick(2), PlayerId(2)), Some(&Action::Idle));
}

#[test]
fn an_absent_entry_is_the_default_action() {
    let log = log();
    assert_eq!(log.get(Tick(11), PlayerId(0)), Some(&Action::Idle));
    assert!(!log.is_confirmed(Tick(11), PlayerId(0)));
}

#[test]
fn a_row_the_log_does_not_cover_is_empty_rather_than_idle() {
    let log = log();
    assert!(log.row(Tick(9)).is_empty());
    assert!(log.row(Tick(14)).is_empty());
    assert_eq!(log.get(Tick(14), PlayerId(0)), None);
    assert_eq!(log.get(Tick(9), PlayerId(0)), None);
}

#[test]
fn four_rows_reach_one_tick_further_than_they_index() {
    // The row at tick T carries the state at T to the state at T + 1, so a log
    // of four rows starting at 10 reaches the state at 14.
    let log = log();
    assert_eq!(log.ticks(), 4);
    assert_eq!(log.last(), Tick(14));
}

#[test]
fn a_correction_that_changes_nothing_is_not_an_error() {
    let mut log = log();
    assert_eq!(log.set(Tick(11), PlayerId(1), Action::Bump), Ok(()));
    assert_eq!(log.set(Tick(11), PlayerId(1), Action::Bump), Ok(()));
    assert_eq!(log.get(Tick(11), PlayerId(1)), Some(&Action::Bump));
}

#[test]
fn a_correction_that_changes_a_confirmed_action_is_an_error() {
    let mut log = log();
    log.set(Tick(11), PlayerId(1), Action::Bump).unwrap();
    assert_eq!(
        log.set(Tick(11), PlayerId(1), Action::Reset),
        Err(Refused::Confirmed {
            tick: Tick(11),
            player: PlayerId(1),
        }),
    );
    // And it is refused rather than half-applied.
    assert_eq!(log.get(Tick(11), PlayerId(1)), Some(&Action::Bump));
}

#[test]
fn a_confirmed_default_cannot_be_overwritten_either() {
    // The neighbouring bug the confirmation bit exists for. Comparing against
    // `A::default()` instead of reading the bit passes every assertion above
    // and accepts this one, because a confirmed idle and an entry nobody ever
    // wrote hold the same bytes.
    let mut log = log();
    log.set(Tick(12), PlayerId(0), Action::Idle).unwrap();
    assert!(log.is_confirmed(Tick(12), PlayerId(0)));
    assert_eq!(
        log.set(Tick(12), PlayerId(0), Action::Bump),
        Err(Refused::Confirmed {
            tick: Tick(12),
            player: PlayerId(0),
        }),
    );
    assert_eq!(log.get(Tick(12), PlayerId(0)), Some(&Action::Idle));
}

#[test]
fn confirming_one_entry_confirms_no_other() {
    // A bitset indexed by the byte rather than by the bit, or by `index` rather
    // than `index / 8`, would confirm a neighbour here.
    let mut log = log();
    log.set(Tick(10), PlayerId(1), Action::Bump).unwrap();
    for tick in 10..=13 {
        for seat in 0..3 {
            let confirmed = log.is_confirmed(Tick(tick), PlayerId(seat));
            assert_eq!(
                confirmed,
                (tick, seat) == (10, 1),
                "tick {tick} seat {seat} reads confirmed as {confirmed}",
            );
        }
    }
}

#[test]
fn the_bitset_reaches_past_the_first_byte() {
    // Eight entries fit in one byte, and a three-seat log's ninth entry is at
    // tick 12 seat 2. A bitset that only ever wrote byte zero would report this
    // one unconfirmed.
    let mut log = log();
    log.set(Tick(12), PlayerId(2), Action::Bump).unwrap();
    assert!(log.is_confirmed(Tick(12), PlayerId(2)));
    assert_eq!(
        log.set(Tick(12), PlayerId(2), Action::Reset),
        Err(Refused::Confirmed {
            tick: Tick(12),
            player: PlayerId(2),
        }),
    );
}

#[test]
fn writing_needs_a_row_and_growing_is_a_separate_decision() {
    let mut log = log();
    assert_eq!(
        log.set(Tick(14), PlayerId(0), Action::Bump),
        Err(Refused::Beyond {
            tick: Tick(14),
            first: Tick(10),
            rows: 4,
        }),
    );
    log.extend_to(Tick(14)).unwrap();
    assert_eq!(log.set(Tick(14), PlayerId(0), Action::Bump), Ok(()));
}

#[test]
fn growing_keeps_what_was_already_there() {
    let mut log = log();
    log.set(Tick(11), PlayerId(2), Action::Reset).unwrap();
    log.extend_to(Tick(40)).unwrap();
    assert_eq!(log.ticks(), 31);
    assert_eq!(log.get(Tick(11), PlayerId(2)), Some(&Action::Reset));
    assert!(log.is_confirmed(Tick(11), PlayerId(2)));
    assert!(!log.is_confirmed(Tick(40), PlayerId(2)));
}

#[test]
fn growing_backwards_is_not_shrinking() {
    let mut log = log();
    log.set(Tick(13), PlayerId(0), Action::Bump).unwrap();
    log.extend_to(Tick(10)).unwrap();
    assert_eq!(log.ticks(), 4);
    assert_eq!(log.get(Tick(13), PlayerId(0)), Some(&Action::Bump));
}

#[test]
fn a_tick_before_the_opening_is_refused_by_both_writers() {
    let mut log = log();
    assert_eq!(
        log.extend_to(Tick(9)),
        Err(Refused::Early {
            tick: Tick(9),
            first: Tick(10),
        }),
    );
    assert_eq!(
        log.set(Tick(9), PlayerId(0), Action::Bump),
        Err(Refused::Early {
            tick: Tick(9),
            first: Tick(10),
        }),
    );
}

#[test]
fn a_seat_the_log_has_no_column_for_is_refused() {
    let mut log = log();
    assert_eq!(
        log.set(Tick(11), PlayerId(3), Action::Bump),
        Err(Refused::Seat {
            player: PlayerId(3),
            players: 3,
        }),
    );
    assert_eq!(log.get(Tick(11), PlayerId(3)), None);
}

#[test]
fn a_row_count_that_does_not_fit_an_index_is_refused() {
    // The reason growing is not something `set` does on demand. Three seats
    // across sixteen quintillion ticks is more entries than an index can count,
    // and the answer is an error rather than an abort.
    let mut log = log();
    assert_eq!(
        log.extend_to(Tick(u64::MAX)),
        Err(Refused::Memory { rows: u64::MAX - 9 }),
    );
    // And the log is untouched, so the session it belongs to is still playable.
    assert_eq!(log.ticks(), 4);
    assert_eq!(log.row(Tick(13)).len(), 3);
}

#[test]
fn a_row_count_that_fits_an_index_and_not_this_machine_is_refused() {
    // The other half, and the one the index arithmetic above cannot reach: at
    // one seat a row is one entry, so this many rows is a number `usize` counts
    // happily and no allocator will ever hand over. `try_reserve` is what turns
    // it into a value rather than an abort — a log grown with `resize` alone
    // takes the process down here.
    let rows = u64::MAX / 2;
    let mut log: ActionLog<Action> = ActionLog::new(Tick::ZERO, 1);
    assert_eq!(log.extend_to(Tick(rows - 1)), Err(Refused::Memory { rows }));
    assert_eq!(log.ticks(), 0);
}

#[test]
fn a_log_with_no_seats_covers_no_ticks() {
    // The wart of the dense layout, pinned rather than left to be discovered: a
    // row of nothing occupies no entries, so there is nothing for the extent to
    // be counted from.
    let mut log: ActionLog<Action> = ActionLog::new(Tick::ZERO, 0);
    log.extend_to(Tick(100)).unwrap();
    assert_eq!(log.ticks(), 0);
    assert_eq!(log.last(), Tick::ZERO);
}

#[test]
fn only_a_write_that_changes_a_stored_action_counts_as_a_correction() {
    // What the generation counts, entry by entry. It is the number a snapshot
    // ring is keyed to, so a count that moved when nothing moved would throw
    // away snapshots for free, and one that stayed still when something moved
    // would hand back a state of a history that did not happen.
    let mut log = log();
    assert_eq!(log.generation(), 0);

    // Confirming the default leaves the actions exactly as they were.
    log.set(Tick(11), PlayerId(0), Action::Idle).unwrap();
    assert!(log.is_confirmed(Tick(11), PlayerId(0)));
    assert_eq!(log.generation(), 0);

    // A real action over an unconfirmed default does not.
    log.set(Tick(11), PlayerId(1), Action::Bump).unwrap();
    assert_eq!(log.generation(), 1);

    // And writing the same value again is the idempotent case: a packet that
    // arrived twice did not contradict anything and must not cost the ring.
    log.set(Tick(11), PlayerId(1), Action::Bump).unwrap();
    assert_eq!(log.generation(), 1);

    // As is a refused contradiction, which changed nothing either.
    assert!(log.set(Tick(11), PlayerId(1), Action::Reset).is_err());
    assert_eq!(log.generation(), 1);
}

#[test]
fn a_correction_counts_against_the_rows_after_it_and_not_the_rows_before() {
    // The state at tick T is built from the rows before T, so that is the range
    // a snapshot at T has to be keyed to. Reading the row at T into it as well
    // would be safe and would also invalidate every snapshot in ordinary play,
    // because a runtime keeps the state at T before it learns what happened on
    // T; `tests/seek.rs` is where that consequence is asserted.
    let mut log = log();
    log.set(Tick(12), PlayerId(0), Action::Bump).unwrap();

    assert_eq!(log.generation_at(Tick(10)), 0);
    assert_eq!(log.generation_at(Tick(11)), 0);
    assert_eq!(log.generation_at(Tick(12)), 0);
    assert_eq!(log.generation_at(Tick(13)), 1);
    assert_eq!(log.generation_at(Tick(14)), 1);

    // Past the last row is the whole count, which is what a state simulated to
    // the frontier depends on.
    assert_eq!(log.generation_at(Tick(9_999)), log.generation());

    // Before the log there are no rows at all, whatever the log has taken.
    assert_eq!(log.generation_at(Tick(9)), 0);
    assert_eq!(log.generation_at(Tick::ZERO), 0);

    // A second correction, earlier, counts against the later rows too.
    log.set(Tick(10), PlayerId(2), Action::Reset).unwrap();
    assert_eq!(log.generation_at(Tick(10)), 0);
    assert_eq!(log.generation_at(Tick(11)), 1);
    assert_eq!(log.generation_at(Tick(13)), 2);
}

#[test]
fn growing_the_log_takes_no_correction() {
    // A row that did not exist a moment ago cannot have contradicted anything,
    // so it carries what the rows before it carry and no snapshot goes stale
    // because a runtime decided to record further ahead.
    let mut log = log();
    log.set(Tick(11), PlayerId(0), Action::Bump).unwrap();
    let before = log.generation();

    log.extend_to(Tick(40)).unwrap();
    assert_eq!(log.generation(), before);
    assert_eq!(log.generation_at(Tick(41)), before);
}
