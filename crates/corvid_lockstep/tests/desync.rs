//! The digest exchange, the halt, and the bisector that names what parted.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::similar_names,
    reason = "two peers of one session are `here` and `there`, and the datagrams between them are named after them"
)]

mod common;

use common::{Action, Swarm, peer, push};
use corvid_behavior::PlayerId;
use corvid_hash::digest;
use corvid_lockstep::{Budget, Desync, Halt};
use corvid_time::Tick;

/// The [`Desync`] a halt carries, if it is one.
fn as_desync(halt: Halt) -> Option<Desync> {
    match halt {
        Halt::Desync(desync) => Some(desync),
        _ => None,
    }
}

/// Runs two peers over a link that loses nothing and delays nothing.
fn perfect(ticks: u64) -> (corvid_lockstep::Peer<Swarm>, corvid_lockstep::Peer<Swarm>) {
    let mut here = peer(16, 2, 0, Budget::DEFAULT);
    let mut there = peer(16, 2, 1, Budget::DEFAULT);

    for at in 0..ticks {
        here.submit(if at.is_multiple_of(5) {
            push(3)
        } else {
            Action::Idle
        })
        .unwrap();
        there
            .submit(if at.is_multiple_of(3) {
                push(-1)
            } else {
                Action::Idle
            })
            .unwrap();

        let (from_here, from_there) = (here.outgoing(), there.outgoing());
        let _ = here.receive(&from_there).unwrap();
        let _ = there.receive(&from_here).unwrap();

        let _ = here.advance(&mut corvid_behavior::Discard::new()).unwrap();
        let _ = there.advance(&mut corvid_behavior::Discard::new()).unwrap();
    }
    (here, there)
}

#[test]
fn two_peers_on_a_perfect_link_agree_on_every_mark() {
    // Every `receive` above compares the mark it (), so reaching the end
    // without a `Halt` is most of this. The traces are the rest.
    let (here, there) = perfect(200);

    assert_eq!(here.tick(), there.tick());
    assert!(here.tick() >= Tick(190), "{:?}", here.tick());
    for at in 0..=here.tick().0 {
        assert_eq!(
            here.session.marks.get(Tick(at)),
            there.session.marks.get(Tick(at)),
            "tick {at}",
        );
    }
    assert!(here.agreed_through() > Tick(150), "and they said so");
}

#[test]
fn a_corrupted_state_is_caught_at_the_tick_it_was_corrupted() {
    let (here, _) = perfect(46);
    let at = Tick(40);

    // What the other peer would have marked if one row of its velocity column
    // had gone wrong at tick 40.
    let (_, mut wrong) = here.restore(at).unwrap();
    wrong.velocity[3] = wrong.velocity[3].wrapping_add(1);
    let corrupt = digest(&wrong);
    assert_ne!(corrupt, here.session.marks.get(at).unwrap());

    let halt = here
        .compare(PlayerId(1), at, corrupt)
        .expect_err("the marks differ");

    let desync = as_desync(halt).expect("a mark that disagrees is a desync");
    // The tick they parted at, rather than the tick the mark arrived on.
    assert_eq!(desync.at, at);
    assert_eq!(desync.peer, PlayerId(1));
    assert_eq!(desync.remote, corrupt);
    assert_eq!(desync.local, here.session.marks.get(at).unwrap());
    assert!(
        desync.fields.is_empty(),
        "a mark comparison names no subsystem; that is what the bisector is for",
    );
    assert!(desync.first_divergent.is_none());
}

#[test]
fn a_mark_that_arrives_in_a_datagram_is_caught_on_the_tick_it_covers() {
    let (mut here, there) = perfect(46);
    let at = Tick(40);
    let (_, mut wrong) = here.restore(at).unwrap();
    wrong.towers[1] = wrong.towers[1].wrapping_add(7);

    // The datagram the other peer would have sent, with the actions it really
    // sent and a mark from a state that went wrong six ticks ago.
    let mut arrived = there.outgoing();
    arrived.marked = at;
    arrived.mark = digest(&wrong);

    let halt = here
        .receive(&arrived)
        .expect_err("the mark it () disagrees");

    let desync = as_desync(halt).expect("a mark that disagrees is a desync");
    assert_eq!(desync.at, at, "not the tick the datagram arrived on");
    assert!(here.tick() > at);
}

#[test]
fn a_resync_request_names_the_last_agreed_tick() {
    let (here, _) = perfect(46);
    let agreed = here.agreed_through();
    assert!(agreed > Tick::ZERO, "the two peers had been agreeing");

    let asked = here.resync_request(Tick(40));

    assert_eq!(asked.seat, PlayerId(0));
    assert_eq!(asked.at, Tick(40));
    assert_eq!(asked.agreed_through, agreed);
}

#[test]
fn adopting_a_transferred_state_resumes_the_session() {
    let (mut here, there) = perfect(46);
    let at = Tick(40);

    // `here` throws its own state away and takes the one that arrived over a
    // reliable channel.
    let (_, transferred) = there.restore(at).unwrap();
    here.adopt(at, transferred.clone()).unwrap();

    assert_eq!(here.tick(), at);
    assert_eq!(digest(here.state()), digest(&transferred));
    assert_eq!(here.session.marks.get(at), there.session.marks.get(at));

    // And the next tick lands where the sender's did.
    let advanced = here.advance(&mut corvid_behavior::Discard::new()).unwrap();
    assert_eq!(advanced.tick, at.next());
    assert_eq!(
        here.session.marks.get(at.next()),
        there.session.marks.get(at.next()),
    );
}
