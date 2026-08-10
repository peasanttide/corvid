//! What the contract is for: two runs of the same session agree tick for tick,
//! and a run that differs anywhere differs from exactly there onwards.
//!
//! These are the properties `corvid_replay` builds rollback, desync detection
//! and the time slider out of. They are asserted here, against the trait, so
//! that a crate above this one that gets them wrong has something to fail
//! against rather than something to discover.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Action, RULES, Rules, active, joining, level, opening};
use corvid_behavior::{Discard, PlayerId, PlayerState, Presence, ProfileId, State as _};
use corvid_hash::{Digest, digest};
use corvid_time::Tick;
/// How many players every session below is played with.
const SEATS: usize = 3;

/// A script that is varied enough that a state which ignored half of it would
/// still be caught, and short enough to read.
///
/// It is generated rather than written out because the interesting length is
/// hundreds of ticks: a divergence that only shows up once the roster has been
/// carried forward a hundred times is exactly the kind this exists to catch.
fn script(ticks: usize) -> Vec<[Action; SEATS]> {
    (0..ticks)
        .map(|tick| {
            let mut actions = [Action::Idle; SEATS];
            for (seat, action) in actions.iter_mut().enumerate() {
                *action = match (tick + seat * 5) % 7 {
                    0 | 3 => Action::Bump,
                    5 => Action::Leave,
                    _ => Action::Idle,
                };
            }
            actions
        })
        .collect()
}

/// Plays a script through and returns one digest per tick, the way a
/// `HashTrace` will.
fn trace(script: &[[Action; SEATS]], rules: &Rules) -> Vec<Digest> {
    let level = level();
    let mut state = opening();
    let mut marks = Vec::with_capacity(script.len());

    for (step, actions) in script.iter().enumerate() {
        // Everyone joins on the opening tick, so the roster column is occupied
        // for the rest of the run and a trace that stopped hashing it would
        // stop matching.
        let players = if step == 0 {
            joining(actions)
        } else {
            active(actions)
        };
        let next = state
            .clone()
            .tick(&level, &players, rules, &mut Discard::new());
        drop(std::mem::replace(&mut state, next));
        marks.push(digest(&state));
    }

    marks
}

#[test]
fn two_runs_of_the_same_session_agree_tick_for_tick() {
    let script = script(500);
    assert_eq!(trace(&script, &RULES), trace(&script, &RULES));
}

#[test]
fn the_trace_is_not_constant() {
    // Every assertion in this file compares traces, and comparing two constant
    // traces proves nothing. This is the check that the fixture has content.
    let marks = trace(&script(500), &RULES);
    let distinct: std::collections::HashSet<Digest> = marks.iter().copied().collect();
    assert!(
        distinct.len() > 400,
        "only {} distinct marks in 500 ticks",
        distinct.len(),
    );
}

#[test]
fn a_changed_action_diverges_at_that_tick_and_not_before() {
    let clean = script(500);
    let mut corrected = clean.clone();
    corrected[137][1] = match corrected[137][1] {
        Action::Bump => Action::Idle,
        _ => Action::Bump,
    };

    let before = trace(&clean, &RULES);
    let after = trace(&corrected, &RULES);

    assert_eq!(before[..137], after[..137], "the past changed");
    assert_ne!(before[137], after[137], "the correction did nothing");
    assert_ne!(before[499], after[499], "the divergence healed itself");
}

#[test]
fn the_rules_are_part_of_the_simulation() {
    // Which is why they are hashed into the opening and `Settings` are not: two
    // peers that disagree here are running two different games.
    let script = script(50);
    assert_ne!(trace(&script, &RULES), trace(&script, &Rules { step: 4 }));
}

#[test]
fn a_roster_hashes_alongside_the_state() {
    // A desync caused by two peers disagreeing about who did what has to be
    // distinguishable from one caused by them disagreeing about what the
    // simulation made of it, so the roster hashes on its own.
    let bump = Action::Bump;
    let idle = Action::Idle;

    let seat = |id: u16, presence, action| {
        vec![PlayerState {
            id: PlayerId(id),
            presence,
            action,
        }]
    };

    let joining = |profile| Presence::Joining {
        profile: ProfileId(profile),
    };

    let base = digest(&seat(0, Presence::Active, bump)[..]);
    assert_eq!(base, digest(&seat(0, Presence::Active, bump)[..]));
    assert_ne!(base, digest(&seat(1, Presence::Active, bump)[..]));
    assert_ne!(base, digest(&seat(0, Presence::Active, idle)[..]));
    assert_ne!(
        base,
        digest(&seat(0, Presence::Dropped { since: Tick(4) }, bump)[..]),
    );
    assert_ne!(
        digest(&seat(0, joining(1), bump)[..]),
        digest(&seat(0, joining(2), bump)[..]),
    );

    // Three fields and three fields only, named exhaustively. `PlayerState` has
    // no pose and must not grow one back: a pose is not in the action log, so a
    // replay would have nothing to rebuild it from and would silently reach a
    // different state than the session ran. Anything added here has to be
    // something a capture can reconstruct, and this pattern is what makes the
    // question unavoidable -- a fourth field stops it compiling.
    let one = seat(0, Presence::Active, bump);
    let PlayerState {
        id: _,
        presence: _,
        action: _,
    } = one[0];
}

#[test]
fn a_presence_discriminates_before_its_payload() {
    // `Active` has no payload and must still absorb a word of its own,
    // otherwise a roster of actives would digest as an empty one.
    assert_ne!(
        digest(&Presence::Active),
        digest(&Presence::Dropped { since: Tick(0) }),
    );
    assert_ne!(
        digest(&Presence::Active),
        digest(&Presence::Joining {
            profile: ProfileId(0),
        }),
    );
    assert_ne!(
        digest(&Presence::Joining {
            profile: ProfileId(0),
        }),
        digest(&Presence::Dropped { since: Tick(0) }),
    );

    // The three assertions above still pass if `Active` absorbs nothing at all,
    // because the other two absorb a payload it does not have. An array is what
    // catches that: it absorbs no length of its own, so a variant that absorbs
    // nothing makes a roster of two actives and a roster of three into the same
    // sequence of words.
    assert_ne!(
        digest(&[Presence::Active; 2]),
        digest(&[Presence::Active; 3]),
    );
}
