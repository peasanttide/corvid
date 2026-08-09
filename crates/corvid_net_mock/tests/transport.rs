//! The trait's contract, asserted against the one backend this crate carries.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use core::time::Duration;

use corvid_fixed::Factor16;

use corvid_net::{Channel, DATAGRAM_LIMIT, Delivery, Lost, PeerId, SendError, Transport};
use corvid_net_mock::{MockNet, Schedule};

use corvid_signal::Seen;
/// One delivery, owned, so a test can compare a whole sequence at once.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Heard {
    Datagram(Vec<u8>),
    Stream(Channel, Vec<u8>),
    Joined,
    Lost(Lost),
}

/// Everything waiting for one peer.
fn drain(link: &dyn Transport) -> Vec<(PeerId, Heard)> {
    let mut heard = Vec::new();
    link.poll(&mut |from, what| {
        let one = match what {
            Delivery::Datagram(bytes) => Heard::Datagram(bytes.to_vec()),
            Delivery::Stream { channel, bytes } => Heard::Stream(channel, bytes.to_vec()),
            Delivery::Joined => Heard::Joined,
            Delivery::Lost { because } => Heard::Lost(because),
            _ => return,
        };
        heard.push((from, one));
    });
    heard
}

#[test]
fn a_datagram_reaches_a_connected_peer() {
    let net = MockNet::new(2, 1);
    let alice = net.endpoint(PeerId(1));
    let bob = net.endpoint(PeerId(2));

    assert_eq!(drain(&bob), [(PeerId(1), Heard::Joined)]);

    alice.send_datagram(PeerId(2), b"tick 41").unwrap();
    net.advance(Duration::from_millis(1));

    assert_eq!(
        drain(&bob),
        [(PeerId(1), Heard::Datagram(b"tick 41".to_vec()))]
    );
}

#[test]
fn a_datagram_to_nobody_is_unknown() {
    let net = MockNet::new(2, 1);
    let alice = net.endpoint(PeerId(1));

    assert_eq!(
        alice.send_datagram(PeerId::NONE, b"hello"),
        Err(SendError::Unknown(PeerId::NONE))
    );
    assert_eq!(
        alice.send_datagram(PeerId(9), b"hello"),
        Err(SendError::Unknown(PeerId(9)))
    );
    // A peer cannot reach itself, either: there is no link there to schedule.
    assert_eq!(
        alice.send_datagram(PeerId(1), b"hello"),
        Err(SendError::Unknown(PeerId(1)))
    );
}

#[test]
fn a_datagram_past_the_limit_reports_both_numbers() {
    let net = MockNet::new(2, 1);
    let alice = net.endpoint(PeerId(1));

    assert_eq!(alice.datagram_limit(), DATAGRAM_LIMIT);
    alice
        .send_datagram(PeerId(2), &vec![0; DATAGRAM_LIMIT])
        .unwrap();

    assert_eq!(
        alice.send_datagram(PeerId(2), &vec![0; DATAGRAM_LIMIT + 1]),
        Err(SendError::TooLarge {
            bytes: DATAGRAM_LIMIT + 1,
            limit: DATAGRAM_LIMIT,
        })
    );
}

#[test]
fn a_stream_arrives_in_order() {
    let net = MockNet::new(2, 1);
    let alice = net.endpoint(PeerId(1));
    let bob = net.endpoint(PeerId(2));
    drain(&bob);

    for frame in 0..6_u8 {
        alice
            .send_stream(PeerId(2), Channel::Control, &[frame])
            .unwrap();
    }

    let mut heard = Vec::new();
    for _ in 0..8 {
        net.advance(Duration::from_millis(1));
        heard.extend(drain(&bob));
    }

    let expected: Vec<_> = (0..6_u8)
        .map(|frame| (PeerId(1), Heard::Stream(Channel::Control, vec![frame])))
        .collect();
    assert_eq!(heard, expected);
}

#[test]
fn a_channel_does_not_hold_up_another() {
    let net = MockNet::new(2, 1);
    net.all(Schedule::new(
        Duration::from_millis(20),
        Duration::ZERO,
        Factor16::ZERO,
        Factor16::ZERO,
    ));
    let alice = net.endpoint(PeerId(1));
    let bob = net.endpoint(PeerId(2));
    drain(&bob);

    for frame in 0..4_u8 {
        alice
            .send_stream(PeerId(2), Channel::Transfer, &[frame])
            .unwrap();
    }
    alice
        .send_stream(PeerId(2), Channel::Chat, b"good luck")
        .unwrap();

    net.advance(Duration::from_millis(21));
    let heard = drain(&bob);

    // The chat line is through while three quarters of the transfer is still
    // behind the frame at its head.
    assert!(heard.contains(&(
        PeerId(1),
        Heard::Stream(Channel::Chat, b"good luck".to_vec())
    )));
    assert_eq!(
        heard
            .iter()
            .filter(|(_, one)| matches!(one, Heard::Stream(Channel::Transfer, _)))
            .count(),
        1
    );
}

#[test]
fn a_join_precedes_and_a_loss_follows() {
    let net = MockNet::new(2, 1);
    let alice = net.endpoint(PeerId(1));
    let bob = net.endpoint(PeerId(2));

    alice.send_datagram(PeerId(2), b"first").unwrap();
    net.advance(Duration::from_millis(1));
    net.cut(PeerId(1), PeerId(2), Lost::TimedOut);

    assert_eq!(
        drain(&bob),
        [
            (PeerId(1), Heard::Joined),
            (PeerId(1), Heard::Datagram(b"first".to_vec())),
            (PeerId(1), Heard::Lost(Lost::TimedOut)),
        ]
    );
}

#[test]
fn a_roster_is_published_once_per_change() {
    let net = MockNet::new(3, 1);
    let alice = net.endpoint(PeerId(1));
    let mut seen = Seen::default();

    // The set that is already there is a change to a cursor that has seen
    // nothing, so a peer starting up mid-session is told who is here.
    let roster = alice.peers().changed_since(&mut seen).unwrap();
    assert_eq!(roster.as_slice(), [PeerId(2), PeerId(3)]);
    assert!(alice.peers().changed_since(&mut seen).is_none());

    net.cut(PeerId(1), PeerId(3), Lost::Closed);
    let roster = alice.peers().changed_since(&mut seen).unwrap();
    assert_eq!(roster.as_slice(), [PeerId(2)]);
    assert!(alice.peers().changed_since(&mut seen).is_none());

    // A link already severed publishes nothing.
    net.cut(PeerId(1), PeerId(3), Lost::Closed);
    assert!(alice.peers().changed_since(&mut seen).is_none());
}

#[test]
fn an_endpoint_for_a_peer_that_is_not_there_reaches_nobody() {
    let net = MockNet::new(2, 1);
    let nobody = net.endpoint(PeerId::NONE);

    assert!(nobody.peers().get().is_empty());
    assert_eq!(
        nobody.send_datagram(PeerId(1), b"hello"),
        Err(SendError::Unknown(PeerId(1)))
    );
    assert_eq!(drain(&nobody), []);
}

#[test]
fn a_sink_can_answer_from_inside_the_loop() {
    let net = MockNet::new(2, 1);
    let alice = net.endpoint(PeerId(1));
    let bob = net.endpoint(PeerId(2));

    alice.send_datagram(PeerId(2), b"ping").unwrap();
    net.advance(Duration::from_millis(1));

    bob.poll(&mut |from, what| {
        if let Delivery::Datagram(bytes) = what {
            assert_eq!(bytes, b"ping");
            bob.send_datagram(from, b"pong").unwrap();
        }
    });
    net.advance(Duration::from_millis(1));

    assert_eq!(
        drain(&alice),
        [
            (PeerId(2), Heard::Joined),
            (PeerId(2), Heard::Datagram(b"pong".to_vec())),
        ]
    );
}
