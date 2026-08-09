//! What the two bounds do when a peer stops reading: the inbox and the pipe.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use core::time::Duration;

use corvid_fixed::Factor16;
use corvid_net::{Channel, Delivery, PeerId, SendError, Transport};
use corvid_net_mock::{INBOX, MockNet, QUEUED, Schedule};

/// A join outlives the flood that buries it.
///
/// [`Delivery::Joined`] promises it precedes everything else from that peer,
/// and the inbox bound is what could break the promise: it drops the oldest,
/// and the oldest is the join. A peer that stops polling for a thousand
/// arrivals is an ordinary thing -- a loading screen, a breakpoint -- and it must
/// not come back to datagrams from someone it was never told about.
#[test]
fn the_inbox_bound_drops_traffic_and_never_a_join() {
    let net = MockNet::new(2, 1);
    net.all(Schedule::PERFECT);
    let alice = net.endpoint(PeerId(1));

    for number in 0..u32::try_from(INBOX).unwrap() + 100 {
        alice
            .send_datagram(PeerId(2), &number.to_le_bytes())
            .unwrap();
        net.advance(Duration::from_millis(1));
    }

    let (mut seen, mut joins) = (0_usize, 0_usize);
    net.endpoint(PeerId(2)).poll(&mut |_from, delivery| {
        seen += 1;
        if matches!(delivery, Delivery::Joined) {
            joins += 1;
        }
    });

    assert_eq!(joins, 1, "the join was evicted by the traffic behind it");

    // `INBOX` datagrams *plus* the join, not `INBOX` arrivals in total. The
    // bound is on traffic alone, and a join sitting on top of a full allowance
    // is the point: counting connection events against it would mean a session
    // with more peers than the bound opened every inbox already over it.
    assert_eq!(seen, INBOX + 1);

    // The counts agree with what the sink could actually see, and the hundred
    // it could not are on `dropped` rather than nowhere. Reporting them as
    // delivered would make `delivered` a number about this network's
    // bookkeeping rather than about the peer; reporting them as neither would
    // leave `sent` unaccounted for.
    let tally = net.tally();
    let sent = u64::try_from(INBOX).unwrap() + 100;
    assert_eq!(tally.sent, sent);
    assert_eq!(
        tally.delivered,
        u64::try_from(INBOX).unwrap(),
        "delivered counts arrivals the sink never got"
    );
    assert_eq!(tally.dropped, 100, "an evicted datagram is a dropped one");
    assert_eq!(tally.delivered + tally.dropped, sent);
}

/// A session with more peers than the inbox bound still carries traffic.
///
/// The bound is on traffic, and this is why. Counting the opening joins
/// against it would put every inbox over the allowance before a byte was sent:
/// each datagram would be evicted the moment it landed, being the only thing
/// there worth dropping, and every reliable frame would be withheld for ever
/// by a window that never reopens. The whole network would go quiet, and only
/// above a peer count nothing in the suite reached.
#[test]
fn a_session_larger_than_the_inbox_bound_is_not_deaf() {
    let peers = u16::try_from(INBOX).unwrap() + 200;
    let net = MockNet::new(peers, 1);
    // One link rather than `all`, which would script `peers` squared of them.
    net.link(PeerId(1), PeerId(2), Schedule::PERFECT);

    let alice = net.endpoint(PeerId(1));
    alice.send_datagram(PeerId(2), b"tick").unwrap();
    alice
        .send_stream(PeerId(2), Channel::Chat, b"frame")
        .unwrap();
    net.advance(Duration::from_millis(1));

    let (mut datagrams, mut frames) = (0_u32, 0_u32);
    net.endpoint(PeerId(2))
        .poll(&mut |_from, delivery| match delivery {
            Delivery::Datagram(_) => datagrams += 1,
            Delivery::Stream { .. } => frames += 1,
            _ => {}
        });

    assert_eq!(
        datagrams, 1,
        "the datagram was evicted by the opening joins"
    );
    assert_eq!(
        frames, 1,
        "the frame was withheld by a window the joins filled"
    );
}

/// A channel that fills refuses rather than growing.
///
/// The refusal is in [`Transport`]'s contract, so a caller has to handle it,
/// and handling that no test can reach is handling that first runs against a
/// real socket.
#[test]
fn a_channel_that_fills_says_so_rather_than_growing() {
    let net = MockNet::new(2, 1);
    // Nothing gets through, so the queue only fills.
    net.all(Schedule::new(
        Duration::from_millis(50),
        Duration::ZERO,
        Factor16::ONE,
        Factor16::ZERO,
    ));
    let alice = net.endpoint(PeerId(1));

    for _ in 0..QUEUED {
        alice
            .send_stream(PeerId(2), Channel::Transfer, b"state")
            .unwrap();
    }

    let refused = alice.send_stream(PeerId(2), Channel::Transfer, b"one too many");
    assert!(
        matches!(
            refused,
            Err(SendError::Backpressure { waiting, limit })
                if waiting == QUEUED && limit == QUEUED
        ),
        "{refused:?}",
    );

    // The other channel is its own queue, which is the point of channels.
    alice
        .send_stream(PeerId(2), Channel::Chat, b"chat")
        .unwrap();
}

/// How many datagrams the reorder test sends.
const SENT: u32 = 20;

/// Reorder moves a datagram across a neighbour, not across the whole burst.
///
/// [`Tally::reordered`] counts one crossing, so a link at [`Factor16::ONE`]
/// must not hand back the run reversed: a mark that walked backwards with
/// every hit would land each datagram a nanosecond before the last, and twenty
/// of them would arrive exactly backwards.
#[test]
fn every_datagram_reordered_is_not_the_whole_run_reversed() {
    let net = MockNet::new(2, 1);
    net.all(Schedule::new(
        Duration::from_millis(40),
        Duration::ZERO,
        Factor16::ZERO,
        Factor16::ONE,
    ));
    let alice = net.endpoint(PeerId(1));

    for number in 0..SENT {
        alice
            .send_datagram(PeerId(2), &number.to_le_bytes())
            .unwrap();
        net.advance(Duration::from_millis(1));
    }

    let mut order = Vec::new();
    for _ in 0..200 {
        net.advance(Duration::from_millis(1));
        net.endpoint(PeerId(2)).poll(&mut |_from, delivery| {
            if let Delivery::Datagram(bytes) = delivery
                && let Ok(number) = <[u8; 4]>::try_from(bytes)
            {
                order.push(u32::from_le_bytes(number));
            }
        });
    }

    assert_eq!(order.len(), SENT as usize);
    let reversed: Vec<u32> = (0..SENT).rev().collect();
    assert_ne!(order, reversed, "the whole run came back backwards");

    // Every one of them crossed the datagram that was in flight when it was
    // sent, which is what `ONE` asks for, and that is the one still at the end.
    assert_eq!(order.last().copied(), Some(0));
}

/// A link is one direction, and the asymmetric case is the one worth having.
///
/// A peer whose uplink is the bad half has actions arriving late while
/// everyone else's arrive on time, and that is not a case `all` can set up.
#[test]
fn the_two_directions_of_a_link_are_scheduled_apart() {
    let net = MockNet::new(2, 1);
    net.link(
        PeerId(1),
        PeerId(2),
        Schedule::new(
            Duration::from_millis(200),
            Duration::ZERO,
            Factor16::ZERO,
            Factor16::ZERO,
        ),
    );
    net.link(
        PeerId(2),
        PeerId(1),
        Schedule::new(
            Duration::from_millis(10),
            Duration::ZERO,
            Factor16::ZERO,
            Factor16::ZERO,
        ),
    );

    net.endpoint(PeerId(1))
        .send_datagram(PeerId(2), b"up")
        .unwrap();
    net.endpoint(PeerId(2))
        .send_datagram(PeerId(1), b"down")
        .unwrap();

    let mut arrived: Vec<(u128, &'static str)> = Vec::new();
    for _ in 0..300 {
        net.advance(Duration::from_millis(1));
        let at = net.elapsed().as_millis();
        net.endpoint(PeerId(1)).poll(&mut |_from, delivery| {
            if matches!(delivery, Delivery::Datagram(b) if b == b"down") {
                arrived.push((at, "down"));
            }
        });
        net.endpoint(PeerId(2)).poll(&mut |_from, delivery| {
            if matches!(delivery, Delivery::Datagram(b) if b == b"up") {
                arrived.push((at, "up"));
            }
        });
    }

    assert_eq!(arrived, [(10, "down"), (200, "up")]);
}

/// A reliable frame is never dropped to make room, however far behind the peer
/// is.
///
/// The inbox bound drops datagrams, and a stream frame is the one arrival it
/// must not touch: `send_stream` promises the frame arrives and arrives in
/// order, so dropping one leaves a hole in a channel whose contract is that
/// there are none -- and the frames after it would still turn up, which is
/// worse than losing the lot. What holds instead is delivery: a full inbox
/// keeps the frame at the head of its pipe until the peer reads.
#[test]
fn a_full_inbox_holds_a_reliable_frame_rather_than_dropping_one() {
    let net = MockNet::new(2, 1);
    net.all(Schedule::PERFECT);
    let alice = net.endpoint(PeerId(1));

    // A frame that lands first and is then buried. The inbox fills behind it,
    // and the arrival the bound reaches for must be a datagram -- this one is
    // the oldest thing in there, so an eviction that took the oldest would take
    // it.
    alice
        .send_stream(PeerId(2), Channel::Chat, b"first")
        .unwrap();
    net.advance(Duration::from_millis(1));

    // Fill the inbox past its bound with droppable traffic, then send a frame
    // the peer must not lose.
    for number in 0..u32::try_from(INBOX).unwrap() + 50 {
        alice
            .send_datagram(PeerId(2), &number.to_le_bytes())
            .unwrap();
        net.advance(Duration::from_millis(1));
    }
    alice
        .send_stream(PeerId(2), Channel::Transfer, b"the state")
        .unwrap();
    for _ in 0..50 {
        net.advance(Duration::from_millis(1));
    }

    // Nothing was polled, so the frame is still owed rather than lost.
    let mut frames = Vec::new();
    net.endpoint(PeerId(2)).poll(&mut |_from, delivery| {
        if let Delivery::Stream { bytes, .. } = delivery {
            frames.push(bytes.to_vec());
        }
    });
    assert_eq!(
        frames,
        [b"first".to_vec()],
        "the buried frame was evicted, or the second was delivered into a full inbox"
    );

    // And now that the peer has read, it arrives.
    for _ in 0..50 {
        net.advance(Duration::from_millis(1));
    }
    let mut arrived = Vec::new();
    net.endpoint(PeerId(2)).poll(&mut |_from, delivery| {
        if let Delivery::Stream { channel, bytes } = delivery {
            arrived.push((channel, bytes.to_vec()));
        }
    });
    assert_eq!(arrived, [(Channel::Transfer, b"the state".to_vec())]);
}
