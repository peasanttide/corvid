//! What a replayed tick is told about who was playing.
//!
//! The other half of `tests/seek.rs`. A seek has to rebuild the roster the
//! original tick saw -- a profile folded in on the tick it joined, a seat absent
//! before it joins and dropped after it leaves, and a dropped seat still handed
//! whatever the log recorded for it. Getting any of these wrong replays to a
//! different state than it ran, which is the failure this crate exists to make
//! impossible.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Action, forward, play, scripted};
use corvid_behavior::{PlayerId, PlayerState, Presence, ProfileId};
use corvid_replay::{ActionLog, Profile, Session, Shape, Snapshots, Unreachable};
use corvid_time::Tick;

/// Enough for every state a five-hundred-tick session produces.
const ROOMY: usize = 1 << 24;

#[test]
fn seats_no_player_id_can_name_are_left_out_rather_than_folded_onto_the_last() {
    // A roster wider than a `PlayerId` is refused by `Session::new` and by
    // `check`, and neither can stop one being assigned to a `pub` field
    // afterwards. So `seek` still has to do something with it, and what it does
    // is stop after `PlayerId(u16::MAX)`, the last seat there is a number for.
    // The alternative -- what saturating a seat index gives -- is every seat past
    // that arriving as another copy of seat 65 535, so the roster below would
    // hand the tick 65 538 players of which three are the same one.
    let mut session = Session::new(common::opening()).unwrap();
    let seat = session.opening.roster[0];
    session.opening.roster = vec![seat; usize::from(u16::MAX) + 3];
    session.log = ActionLog::new(session.opening.first, u16::MAX);
    session.log.extend_to(session.opening.first).unwrap();
    assert_eq!(
        session.check(),
        Err(Shape::Roster {
            seats: usize::from(u16::MAX) + 3,
        }),
    );

    // Every seat in this roster joined on the opening tick, so the state's
    // roster column holds one profile per player the tick was handed: seats
    // `PlayerId(0)` through `PlayerId(65_535)` and nothing after them. The last
    // of those has no column in a log 65 535 wide and is handed
    // `Action::default()`, which is what a seek does with any seat the log is
    // short of.
    let mut snapshots = Snapshots::new(0);
    let (state, _) = session.seek(&mut snapshots, Tick(1)).unwrap();
    assert_eq!(state.roster.len(), usize::from(u16::MAX) + 1);
}

#[test]
fn a_tick_outside_the_log_is_named_rather_than_clamped() {
    let session = play(10);
    let mut snapshots = Snapshots::new(ROOMY);

    assert_eq!(
        session
            .seek(&mut snapshots, Tick(11))
            .map(|(state, _)| state)
            .unwrap_err(),
        Unreachable::After {
            to: Tick(11),
            last: Tick(10),
        },
    );

    // Tick 10 is reachable and tick 11 is not, which is the boundary a log of
    // ten rows draws. A seek that clamped would return the state at 10 for
    // both and a replay would silently stop short.
    assert!(session.seek(&mut snapshots, Tick(10)).is_ok());
}

#[test]
fn a_tick_before_the_opening_is_named_rather_than_clamped() {
    let mut session = play(0);
    session.opening.first = Tick(50);
    session.log = ActionLog::new(Tick(50), common::seats(&session));

    let mut snapshots = Snapshots::new(ROOMY);
    assert_eq!(
        session
            .seek(&mut snapshots, Tick(49))
            .map(|(state, _)| state)
            .unwrap_err(),
        Unreachable::Before {
            to: Tick(49),
            first: Tick(50),
        },
    );
}

#[test]
fn seeking_to_the_opening_returns_the_opening_state() {
    let session = play(20);
    let mut snapshots = Snapshots::new(ROOMY);
    let (state, _) = session.seek(&mut snapshots, Tick::ZERO).unwrap();
    assert_eq!(state, session.opening.origin());

    // And it is the opening's own handle rather than a copy of it, which is the
    // one case a seek can answer without touching the allocator at all.
    assert!(std::sync::Arc::ptr_eq(&state, &session.opening.origin()));
}

#[test]
fn a_replay_folds_a_joining_profile_in_on_the_tick_the_session_did() {
    // The roster's third seat joins at tick 7 and its fourth at 120, and the
    // state's roster column only moves on a `Presence::Joining` tick. A replay
    // that handed every seat `Presence::Active` -- or that offered a seat before
    // it joined -- would get a different column here, and the digest comparison
    // in the first test would catch it only because of this column.
    let session = play(200);
    let mut snapshots = Snapshots::new(ROOMY);

    // The state *at* tick 8 is what the tick at 7 produced, and 7 is the tick
    // seat 2 is `Joining` on -- so the column moves between 7 and 8, not between
    // 6 and 7. An off-by-one either way in how a row is dated shows up here.
    for (tick, expected) in [(7, 2), (8, 3), (120, 3), (121, 4)] {
        let (state, _) = session.seek(&mut snapshots, Tick(tick)).unwrap();
        assert_eq!(
            state.roster.len(),
            expected,
            "tick {tick} folded in {:?}",
            state.roster,
        );
    }
}

#[test]
fn a_seat_is_absent_before_it_joins_and_dropped_after_it_leaves() {
    // What a replay reconstructs presence from, on its own. There is no
    // "absent" `Presence`, so a seat that has not arrived is left out of the
    // slice the tick sees rather than handed a fourth state to reason about.
    let seat = Profile {
        account: ProfileId(9),
        joined: Tick(10),
        left: Some(Tick(20)),
    };
    assert_eq!(seat.presence_at(Tick(9)), None);
    assert_eq!(
        seat.presence_at(Tick(10)),
        Some(Presence::Joining {
            profile: ProfileId(9),
        }),
    );
    assert_eq!(seat.presence_at(Tick(11)), Some(Presence::Active));
    assert_eq!(seat.presence_at(Tick(19)), Some(Presence::Active));
    assert_eq!(
        seat.presence_at(Tick(20)),
        Some(Presence::Dropped { since: Tick(20) }),
    );
    assert_eq!(
        seat.presence_at(Tick(9_000)),
        Some(Presence::Dropped { since: Tick(20) }),
    );

    // A seat that joined and left on the same tick is dropped rather than
    // joining, because a state that folded its profile in would be folding in a
    // player who was never there.
    let flicker = Profile {
        account: ProfileId(9),
        joined: Tick(10),
        left: Some(Tick(10)),
    };
    assert_eq!(
        flicker.presence_at(Tick(10)),
        Some(Presence::Dropped { since: Tick(10) }),
    );
}

#[test]
fn a_dropped_seat_is_still_handed_what_the_log_records() {
    // The log is the game, and `seek` does not second-guess it. A runtime
    // submits `Action::default()` on a dropped player's behalf, so a real log
    // holds defaults there; this fixture writes scripted actions for every seat
    // on every tick, and the replay applies them. That is deliberate -- a `seek`
    // that substituted defaults for a dropped seat would be a second source of
    // truth beside the log, and two peers whose rosters disagreed about a
    // `left` tick would then disagree about the simulation as well.
    let session = play(400);
    let (states, _) = forward(&session);
    let moved_after_leaving = (301..400).any(|tick| states[tick].movers.contains(&PlayerId(1)));
    assert!(
        moved_after_leaving,
        "seat 1 left at tick 300 and the log still records its actions",
    );
}

#[test]
fn the_ring_is_what_decides_how_much_a_seek_re_simulates() {
    // The claim `Snapshots` and `seek` both document, and the only one the
    // states themselves cannot carry: every seek here returns the same value,
    // so the evidence that the ring does anything is the number of ticks that
    // produced it. A budget of zero replays the whole session; a warm ring
    // replays what is left after the nearest entry.
    let session = play(100);

    let mut cold = Snapshots::new(0);
    let (from_cold, replayed) = session.seek(&mut cold, Tick(100)).unwrap();
    assert_eq!(replayed, 100, "a ring with no budget replays everything");

    let mut warm = Snapshots::new(ROOMY);
    let (_, to_sixty) = session.seek(&mut warm, Tick(60)).unwrap();
    assert_eq!(to_sixty, 60, "still cold on the first seek");
    let (from_warm, to_a_hundred) = session.seek(&mut warm, Tick(100)).unwrap();
    assert_eq!(
        to_a_hundred, 40,
        "the second seek starts from what the first left in the ring",
    );

    // Same answer either way, which is what makes the ring a cache rather than
    // a second implementation.
    assert_eq!(from_cold, from_warm);
}

#[test]
fn every_input_a_tick_sees_is_one_a_replay_can_rebuild() {
    // The pattern below names every field a `PlayerState` has and binds each to
    // where a replay gets it from. It is exhaustive on purpose: a `PlayerState` that
    // grew a fourth field would stop this compiling, and whoever added it would
    // have to answer "where does `seek` get this from" before the suite is
    // green again. That question is the whole of the rule -- an input a capture
    // cannot rebuild makes the session irreproducible, silently.
    let session = play(3);
    let idle = Action::default();
    let seat = PlayerId(0);
    let profile = session.opening.seat(seat).unwrap();

    let player = PlayerState {
        id: seat,
        presence: profile.presence_at(Tick(1)).unwrap(),
        action: session.log.get(Tick(1), seat).unwrap_or(&idle),
    };
    let PlayerState {
        id: from_the_roster_order,
        presence: from_the_rosters_join_and_leave_ticks,
        action: from_the_log,
    } = player;

    assert_eq!(from_the_roster_order, seat);
    assert_eq!(from_the_rosters_join_and_leave_ticks, Presence::Active);
    assert_eq!(from_the_log, &scripted(Tick(1), seat));
}
