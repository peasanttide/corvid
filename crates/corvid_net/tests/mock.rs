//! Every curve `MockNet` follows, and the determinism underneath all of them.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use core::time::Duration;

use corvid_fixed::Factor16;
use corvid_hash::{Digest, digest};
use corvid_net::{Channel, Delivery, Lost, MockNet, PeerId, Schedule, SendError, Transport};
/// The seed the determinism test runs from.
const SEED: u64 = 0x00c0_ffee;

/// A different one, which must not produce the same run.
const OTHER: u64 = 0x00c0_ffef;

/// One delivery, owned and hashable, so a whole run compares as a value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Heard {
    Datagram(Vec<u8>),
    Stream(Channel, Vec<u8>),
    Joined,
    Lost(Lost),
}

/// What one peer heard, and when.
type Trace = Vec<(Duration, PeerId, Heard)>;

/// Everything waiting for one peer, stamped with the network's clock.
fn drain(net: &MockNet, link: &dyn Transport) -> Trace {
    let at = net.elapsed();
    let mut heard = Vec::new();
    link.poll(&mut |from, what| {
        let one = match what {
            Delivery::Datagram(bytes) => Heard::Datagram(bytes.to_vec()),
            Delivery::Stream { channel, bytes } => Heard::Stream(channel, bytes.to_vec()),
            Delivery::Joined => Heard::Joined,
            Delivery::Lost { because } => Heard::Lost(because),
            _ => return,
        };
        heard.push((at, from, one));
    });
    heard
}

/// The payload a datagram carries: which one it was.
fn numbered(index: u32) -> Vec<u8> {
    index.to_le_bytes().to_vec()
}

/// Which one it was, read back.
fn number(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().unwrap())
}

/// Every datagram peer 1 heard, in arrival order, for a run of `count`
/// datagrams sent one per step.
fn run(schedule: Schedule, seed: u64, count: u32, step: Duration) -> Vec<u32> {
    let net = MockNet::new(2, seed);
    net.all(schedule);
    let alice = net.endpoint(PeerId(0));
    let bob = net.endpoint(PeerId(1));

    let mut arrived = Vec::new();
    for index in 0..count {
        alice.send_datagram(PeerId(1), &numbered(index)).unwrap();
        net.advance(step);
        arrived.extend(
            drain(&net, &bob)
                .into_iter()
                .filter_map(|(_, _, one)| match one {
                    Heard::Datagram(bytes) => Some(number(&bytes)),
                    _ => None,
                }),
        );
    }

    // Long enough for anything still in flight on the worst curve here.
    for _ in 0..100 {
        net.advance(step);
        arrived.extend(
            drain(&net, &bob)
                .into_iter()
                .filter_map(|(_, _, one)| match one {
                    Heard::Datagram(bytes) => Some(number(&bytes)),
                    _ => None,
                }),
        );
    }
    arrived
}

#[test]
fn a_perfect_link_delivers_everything_in_send_order() {
    let net = MockNet::new(2, 1);
    let alice = net.endpoint(PeerId(0));
    let bob = net.endpoint(PeerId(1));
    drain(&net, &bob);

    for index in 0..64 {
        alice.send_datagram(PeerId(1), &numbered(index)).unwrap();
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
    let alice = net.endpoint(PeerId(0));
    let bob = net.endpoint(PeerId(1));
    drain(&net, &bob);

    alice.send_datagram(PeerId(1), b"now").unwrap();
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
        "dropped {dropped} of ten thousand, which is past two per cent of half"
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
    let alice = net.endpoint(PeerId(0));
    let bob = net.endpoint(PeerId(1));
    drain(&net, &bob);

    for index in 0..500 {
        alice.send_datagram(PeerId(1), &numbered(index)).unwrap();
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

/// A script both peers talk over, so determinism is asserted over two
/// directions rather than one.
fn conversation(schedule: Schedule, seed: u64) -> Digest {
    let net = MockNet::new(3, seed);
    net.all(schedule);
    let peers: Vec<_> = (0..3).map(|it| net.endpoint(PeerId(it))).collect();

    let mut trace: Trace = Vec::new();
    for tick in 0..300_u32 {
        for (me, link) in peers.iter().enumerate() {
            for to in 0..3_u16 {
                if usize::from(to) != me {
                    let _ = link.send_datagram(PeerId(to), &numbered(tick));
                }
            }
            if tick.is_multiple_of(50) {
                let _ = link.send_stream(PeerId((to_u16(me) + 1) % 3), Channel::Chat, b"mark");
            }
        }

        net.advance(Duration::from_millis(15));

        for link in &peers {
            trace.extend(drain(&net, link));
        }

        if tick == 150 {
            net.cut(PeerId(1), PeerId(2), Lost::Reset);
        }
    }

    digest(&(trace, net.tally()))
}

/// The seat number, as the payloads and the roster both spell it.
fn to_u16(seat: usize) -> u16 {
    u16::try_from(seat).unwrap()
}

#[test]
fn the_same_seed_and_script_deliver_the_same_run() {
    for schedule in [Schedule::PERFECT, Schedule::DOMESTIC, Schedule::MOBILE] {
        let once = conversation(schedule, SEED);
        assert_eq!(once, conversation(schedule, SEED));
    }

    // A perfect link draws nothing it can act on, so its run does not depend
    // on the seed. A link that lies does, and the seed is the whole of what it
    // lies from.
    assert_eq!(
        conversation(Schedule::PERFECT, SEED),
        conversation(Schedule::PERFECT, OTHER)
    );
    assert_ne!(
        conversation(Schedule::DOMESTIC, SEED),
        conversation(Schedule::DOMESTIC, OTHER)
    );
    assert_ne!(
        conversation(Schedule::MOBILE, SEED),
        conversation(Schedule::MOBILE, OTHER)
    );
}

#[test]
fn a_cut_is_told_once_and_refuses_everything_after() {
    let net = MockNet::new(2, 1);
    net.all(Schedule::DOMESTIC);
    let alice = net.endpoint(PeerId(0));
    let bob = net.endpoint(PeerId(1));
    drain(&net, &bob);
    drain(&net, &alice);

    alice.send_datagram(PeerId(1), b"in flight").unwrap();
    alice
        .send_stream(PeerId(1), Channel::Transfer, b"also in flight")
        .unwrap();
    net.cut(PeerId(0), PeerId(1), Lost::Reset);

    let both: Vec<_> = [
        drain(&net, &alice),
        drain(&net, &bob),
        {
            net.advance(Duration::from_millis(500));
            drain(&net, &alice)
        },
        drain(&net, &bob),
    ]
    .concat();

    // One loss each, and nothing that was in flight when the link went.
    assert_eq!(
        both.iter()
            .map(|(_, _, one)| one.clone())
            .collect::<Vec<_>>(),
        [Heard::Lost(Lost::Reset), Heard::Lost(Lost::Reset)]
    );
    assert_eq!(net.tally().in_flight, 0);

    assert_eq!(
        alice.send_datagram(PeerId(1), b"anyone there"),
        Err(SendError::Unknown(PeerId(1)))
    );
    assert_eq!(
        bob.send_stream(PeerId(0), Channel::Control, b"anyone there"),
        Err(SendError::Unknown(PeerId(0)))
    );

    // Cutting again says nothing, because there is nothing left to sever.
    net.cut(PeerId(0), PeerId(1), Lost::Closed);
    assert_eq!(drain(&net, &alice), []);
    assert_eq!(drain(&net, &bob), []);
}

#[test]
fn a_stream_over_a_half_lost_link_arrives_whole_and_in_order() {
    let net = MockNet::new(2, 0xa11);
    net.all(Schedule::new(
        Duration::from_millis(50),
        Duration::from_millis(10),
        Factor16::from_f64(0.5),
        Factor16::from_f64(0.5),
    ));
    let alice = net.endpoint(PeerId(0));
    let bob = net.endpoint(PeerId(1));
    drain(&net, &bob);

    let state: Vec<Vec<u8>> = (0..32_u32).map(numbered).collect();
    for frame in &state {
        alice
            .send_stream(PeerId(1), Channel::Transfer, frame)
            .unwrap();
    }

    let mut arrived = Vec::new();
    let mut steps = 0;
    while arrived.len() < state.len() && steps < 2_000 {
        net.advance(Duration::from_millis(10));
        steps += 1;
        arrived.extend(
            drain(&net, &bob)
                .into_iter()
                .filter_map(|(_, _, one)| match one {
                    Heard::Stream(Channel::Transfer, bytes) => Some(bytes),
                    _ => None,
                }),
        );
    }

    assert_eq!(arrived, state);
    assert!(net.tally().dropped > 0, "a half-lost link lost nothing");
    assert_eq!(net.tally().in_flight, 0);

    // Head-of-line blocking is what makes a transfer over a bad link take a
    // visible amount of time: thirty-two frames of fifty milliseconds each
    // cannot have arrived in less than that.
    assert!(net.elapsed() >= Duration::from_millis(50 * 32));
}
