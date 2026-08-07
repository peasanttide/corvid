//! The one decision a runtime with no server has to make: when somebody left.
//!
//! [`Departures`](corvid_app::Departures) is the agreement, and it is a
//! separate testable value rather than a few fields inside the loop for exactly
//! this reason — the property that matters is about *sets of opinions*, and a
//! test of it should not need three threads, three sockets and a game.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

use corvid_app::Departures;

use corvid_behavior::PlayerId;

use corvid_time::Tick;
/// Seat zero, seat one, seat two.
const A: PlayerId = PlayerId(0);
const B: PlayerId = PlayerId(1);
const C: PlayerId = PlayerId(2);

/// **Nothing is applied until everybody has spoken.**
///
/// This is the whole point. A machine that acted on the first opinion it heard
/// would simulate a roster the other survivor did not have, and the two would
/// send each other digests of states neither could reproduce — which every peer
/// in this workspace reports as a desync, because from the inside that is
/// exactly what it looks like.
#[test]
fn one_opinion_is_not_an_agreement() {
    let mut departures = Departures::new(3);
    assert_eq!(departures.propose(C, A, Tick(40)), None);
    assert_eq!(departures.agreed(C), None);
    assert!(departures.is_live(C), "a seat left on one machine's say-so");
}

/// And the moment the set is complete, it is.
#[test]
fn the_last_opinion_completes_it() {
    let mut departures = Departures::new(3);
    assert_eq!(departures.propose(C, A, Tick(40)), None);
    assert_eq!(
        departures.propose(C, B, Tick(52)),
        Some(Tick(40)),
        "the agreed tick is not the earliest of what was said",
    );
    assert_eq!(departures.agreed(C), Some(Tick(40)));
    assert!(!departures.is_live(C));
}

/// The order the opinions arrive in cannot change the answer.
///
/// Two machines hear each other's proposals in whichever order the network
/// delivers them, so an agreement that depended on the order would be an
/// agreement that depended on the network — which is the thing being agreed
/// about.
#[test]
fn the_order_they_arrive_in_changes_nothing() {
    let ticks = [Tick(31), Tick(40), Tick(52)];
    let mut answers = Vec::new();
    for first in 0..3 {
        for second in 0..3 {
            for third in 0..3 {
                if first == second || second == third || first == third {
                    continue;
                }
                let mut departures = Departures::new(4);
                let mut agreed = None;
                for (seat, at) in [(A, ticks[first]), (B, ticks[second]), (C, ticks[third])] {
                    if let Some(answer) = departures.propose(PlayerId(3), seat, at) {
                        agreed = Some(answer);
                    }
                }
                answers.push(agreed);
            }
        }
    }
    assert!(
        answers.iter().all(|answer| *answer == Some(Tick(31))),
        "the agreed tick depended on the order the opinions arrived in: {answers:?}",
    );
}

/// A machine that says the same thing twice is one machine.
///
/// Control frames are retransmitted until they are acknowledged, so a duplicate
/// is ordinary — and a set that counted one machine twice would complete
/// without the other having said anything.
#[test]
fn a_machine_repeating_itself_does_not_complete_the_set() {
    let mut departures = Departures::new(3);
    assert_eq!(departures.propose(C, A, Tick(40)), None);
    assert_eq!(
        departures.propose(C, A, Tick(41)),
        None,
        "one machine's two opinions completed a set of two machines",
    );
    assert_eq!(departures.propose(C, B, Tick(60)), Some(Tick(40)));
}

/// Nobody waits for an opinion from somebody who has already gone.
///
/// Two machines out of four leave. The survivors' set for the second one is the
/// two of them, because the seat that left first is not going to say anything
/// about anybody.
#[test]
fn a_seat_that_has_left_is_not_waited_for() {
    let mut departures = Departures::new(4);
    assert_eq!(departures.propose(C, A, Tick(10)), None);
    assert_eq!(departures.propose(C, B, Tick(12)), None);
    assert_eq!(departures.propose(C, PlayerId(3), Tick(14)), Some(Tick(10)));

    // And now seat three goes too. Only A and B are left to have an opinion.
    assert_eq!(departures.propose(PlayerId(3), A, Tick(30)), None);
    assert_eq!(
        departures.propose(PlayerId(3), B, Tick(33)),
        Some(Tick(30)),
        "the survivors waited for an opinion from a seat that had already left",
    );
}

/// A departure is applied once, however many times it is heard about.
#[test]
fn an_agreed_departure_is_not_agreed_again() {
    let mut departures = Departures::new(2);
    assert_eq!(departures.propose(B, A, Tick(9)), Some(Tick(9)));
    assert_eq!(
        departures.propose(B, A, Tick(4)),
        None,
        "an earlier opinion arriving late re-applied a departure",
    );
    assert_eq!(departures.agreed(B), Some(Tick(9)));
}

/// Two seats, which is the case `examples/pong` plays: one machine's opinion is
/// the whole set, because there is nobody else to ask.
#[test]
fn two_seats_agree_as_soon_as_the_survivor_says_so() {
    let mut departures = Departures::new(2);
    assert_eq!(departures.propose(B, A, Tick(88)), Some(Tick(88)));
    assert_eq!(departures.all().collect::<Vec<_>>(), [(B, Tick(88))]);
}
