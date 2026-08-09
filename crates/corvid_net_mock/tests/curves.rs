//! What each knob does on its own: order, loss, jitter and reorder.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use core::time::Duration;

use corvid_fixed::Factor16;
use corvid_net::{PeerId, Transport};
use corvid_net_mock::{MockNet, Schedule};

use crate::common::{Heard, drain, number, numbered, run};

#[test]
fn a_perfect_link_delivers_everything_in_send_order() {
    let net = MockNet::new(2, 1);
    let alice = net.endpoint(PeerId(1));
    let bob = net.endpoint(PeerId(2));
    drain(&net, &bob);

    for index in 0..64 {
        alice.send_datagram(PeerId(2), &numbered(index)).unwrap();
    }
    net.advance(Duration::from_millis(1));

    let arrived: Vec<_> = drain(&net, &bob)
        .into_iter()
        .filter_map(|(_, _, one)| match one {
            Heard::Datagram(bytes) => Some(number(&bytes)),
            _ => None,
        })
        .collect();

    assert_eq!(arrived, (0..64).collect::<Vec<_>>());

    let tally = net.tally();
    assert_eq!(tally.dropped, 0);
    assert_eq!(tally.reordered, 0);
    assert_eq!(tally.sent, 64);
    assert_eq!(tally.delivered, 64);
    assert_eq!(tally.in_flight, 0);
}

#[test]
fn an_advance_of_no_time_delivers_nothing() {
    let net = MockNet::new(2, 1);
    let alice = net.endpoint(PeerId(1));
    let bob = net.endpoint(PeerId(2));
    drain(&net, &bob);

    alice.send_datagram(PeerId(2), b"now").unwrap();
    let before = net.tally();

    net.advance(Duration::ZERO);

    assert_eq!(net.tally(), before);
    assert_eq!(net.elapsed(), Duration::ZERO);
    assert_eq!(drain(&net, &bob), []);
    assert_eq!(before.in_flight, 1);
}

#[test]
fn half_of_ten_thousand_is_lost_and_the_same_half_twice() {
    let half = Schedule::new(
        Duration::from_millis(10),
        Duration::ZERO,
        Factor16::from_f64(0.5),
        Factor16::ZERO,
    );

    let first = run(half, 0xf00d, 10_000, Duration::from_millis(1));
    let dropped = 10_000 - first.len();
    assert!(
        dropped.abs_diff(5_000) <= 200,
        "dropped {dropped} of ten thousand, which is past four per cent of half"
    );

    // The same seed loses the same datagrams, not merely as many of them.
    assert_eq!(first, run(half, 0xf00d, 10_000, Duration::from_millis(1)));
    assert_ne!(first, run(half, 0xbeef, 10_000, Duration::from_millis(1)));

    // And a share of one loses all of them, which is the end an inequality
    // gets wrong.
    let all = Schedule::new(
        Duration::from_millis(10),
        Duration::ZERO,
        Factor16::ONE,
        Factor16::ZERO,
    );
    assert_eq!(run(all, 0xf00d, 500, Duration::from_millis(1)), []);
}

#[test]
fn jitter_lands_inside_the_window_and_never_outside() {
    let step = Duration::from_millis(1);
    let latency = Duration::from_millis(40);
    let jitter = Duration::from_millis(10);

    let net = MockNet::new(2, 0x1c3);
    net.all(Schedule::new(
        latency,
        jitter,
        Factor16::ZERO,
        Factor16::ZERO,
    ));
    let alice = net.endpoint(PeerId(1));
    let bob = net.endpoint(PeerId(2));
    drain(&net, &bob);

    for index in 0..500 {
        alice.send_datagram(PeerId(2), &numbered(index)).unwrap();
    }

    let mut arrivals = Vec::new();
    for _ in 0..100 {
        net.advance(step);
        arrivals.extend(drain(&net, &bob).into_iter().map(|(at, _, _)| at));
    }

    assert_eq!(arrivals.len(), 500);
    let earliest = arrivals.iter().min().copied().unwrap();
    let latest = arrivals.iter().max().copied().unwrap();
    assert!(
        earliest >= latency,
        "arrived at {earliest:?}, before the floor"
    );
    assert!(
        latest <= latency + jitter,
        "arrived at {latest:?}, past the ceiling"
    );
    // The window is used rather than merely respected.
    assert!(latest.saturating_sub(earliest) >= jitter / 2);
}

#[test]
fn reorder_puts_a_datagram_out_of_order_and_zero_never_does() {
    let curve = |reorder| {
        Schedule::new(
            Duration::from_millis(40),
            Duration::ZERO,
            Factor16::ZERO,
            reorder,
        )
    };

    let inversions = |arrived: &[u32]| arrived.windows(2).filter(|pair| pair[0] > pair[1]).count();

    let shuffled = run(
        curve(Factor16::from_f64(0.01)),
        3,
        1_000,
        Duration::from_millis(1),
    );
    assert_eq!(shuffled.len(), 1_000);
    assert!(
        inversions(&shuffled) >= 1,
        "a hundredth of a thousand produced no out-of-order arrival at all"
    );

    let orderly = run(curve(Factor16::ZERO), 3, 1_000, Duration::from_millis(1));
    assert_eq!(orderly, (0..1_000).collect::<Vec<_>>());
    assert_eq!(inversions(&orderly), 0);
}
