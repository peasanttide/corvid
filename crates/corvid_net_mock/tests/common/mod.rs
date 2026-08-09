//! What the three suites next door share: a trace, a numbering, and the two
//! drivers that turn a schedule into a list of what arrived.
//!
//! A `tests/common/mod.rs` rather than a fourth test binary -- Cargo builds
//! every top-level file in `tests/` as its own crate, and a directory is how
//! one says "shared, not a suite".

#![allow(dead_code, reason = "each suite uses a subset of these")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "this module is private, so pub(crate) and pub are equivalent -- pub(crate) is the one unreachable_pub asks for, and the lib allows it for the same reason"
)]

use core::time::Duration;

use corvid_hash::{Digest, digest};
use corvid_net::{Channel, Delivery, Lost, PeerId, Transport};
use corvid_net_mock::{MockNet, Schedule};

/// The seed the determinism test runs from.
pub(crate) const SEED: u64 = 0x00c0_ffee;

/// A different one, which must not produce the same run.
pub(crate) const OTHER: u64 = 0x00c0_ffef;

/// One delivery, owned and hashable, so a whole run compares as a value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Heard {
    Datagram(Vec<u8>),
    Stream(Channel, Vec<u8>),
    Joined,
    Lost(Lost),
}

/// What one peer heard, and when.
pub(crate) type Trace = Vec<(Duration, PeerId, Heard)>;

/// Everything waiting for one peer, stamped with the network's clock.
pub(crate) fn drain(net: &MockNet, link: &dyn Transport) -> Trace {
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
pub(crate) fn numbered(index: u32) -> Vec<u8> {
    index.to_le_bytes().to_vec()
}

/// Which one it was, read back.
pub(crate) fn number(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().unwrap())
}

/// Every datagram peer 1 heard, in arrival order, for a run of `count`
/// datagrams sent one per step.
pub(crate) fn run(schedule: Schedule, seed: u64, count: u32, step: Duration) -> Vec<u32> {
    let net = MockNet::new(2, seed);
    net.all(schedule);
    let alice = net.endpoint(PeerId(1));
    let bob = net.endpoint(PeerId(2));

    let mut arrived = Vec::new();
    for index in 0..count {
        alice.send_datagram(PeerId(2), &numbered(index)).unwrap();
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

/// A script both peers talk over, so determinism is asserted over two
/// directions rather than one.
pub(crate) fn conversation(schedule: Schedule, seed: u64) -> Digest {
    let net = MockNet::new(3, seed);
    net.all(schedule);
    let seats = [PeerId(1), PeerId(2), PeerId(3)];
    let peers: Vec<_> = seats.iter().map(|&it| net.endpoint(it)).collect();

    let mut trace: Trace = Vec::new();
    for tick in 0..300_u32 {
        for (me, link) in peers.iter().enumerate() {
            for &to in &seats {
                if to != seats[me] {
                    let _ = link.send_datagram(to, &numbered(tick));
                }
            }
            if tick.is_multiple_of(50) {
                let _ = link.send_stream(seats[(me + 1) % 3], Channel::Chat, b"mark");
            }
        }

        net.advance(Duration::from_millis(15));

        for link in &peers {
            trace.extend(drain(&net, link));
        }

        if tick == 150 {
            net.cut(PeerId(2), PeerId(3), Lost::Reset);
        }
    }

    digest(&(trace, net.tally()))
}

/// The seat number, as the payloads and the roster both spell it.
pub(crate) fn to_u16(seat: usize) -> u16 {
    u16::try_from(seat).unwrap()
}
