//! A session over two real sockets.
//!
//! `tests/session.rs` and `tests/linked.rs` both play over a `MockNet`, which
//! is a real implementation of the transport trait and shares an address space.
//! This one binds two UDP sockets, tells each where the other is, and plays
//! pong across them — so every datagram in it goes through the operating
//! system's networking stack and comes back out.
//!
//! What it cannot say anything about is loss: loopback does not lose packets.
//! That is `corvid_net`'s `tests/reliable.rs` and this crate's
//! `tests/session.rs`, and it is why all three exist.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "every test here returns a `Result` so that a failure reaches for `?` rather than unwrapping, and asserts as well — a failed assertion in a test is a failed test, which is what a test is for"
)]

use std::{
    thread,
    time::{Duration, Instant},
};

use corvid::PlayerId;

use corvid::Controller;
use corvid::Tick;
use corvid::digest as mark_of;
use corvid_lockstep::{Budget, Datagram, Peer};
use corvid_net::{Delivery, PeerId, Transport, udp::UdpNet};
use corvid_replay::Session;
use pong::{Move, Table, opening, rally::Policy};

/// Whatever the test needs to say went wrong.
type Fallible = Result<(), Box<dyn std::error::Error>>;

/// How long the sockets are given to find each other.
const PATIENCE: Duration = Duration::from_secs(5);

/// How many ticks the session plays.
const TICKS: u64 = 300;

/// Two sockets on loopback that have found each other.
fn met() -> Result<(UdpNet, UdpNet), Box<dyn std::error::Error>> {
    let here = UdpNet::bind(("127.0.0.1", 0), PeerId(0))?;
    let there = UdpNet::bind(("127.0.0.1", 0), PeerId(1))?;
    here.connect(PeerId(1), there.local()?)?;
    there.connect(PeerId(0), here.local()?)?;

    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        here.poll(&mut |_, _| {});
        there.poll(&mut |_, _| {});
        if !here.peers().get().is_empty() && !there.peers().get().is_empty() {
            return Ok((here, there));
        }
        thread::sleep(Duration::from_millis(2));
    }
    Err("two sockets on loopback did not find each other".into())
}

/// One peer's tick over a real socket: decide, submit, receive, send, advance.
fn step(
    peer: &mut Peer<Table>,
    socket: &UdpNet,
    other: PeerId,
) -> Result<(), Box<dyn std::error::Error>> {
    let action = Policy::Chase.action(corvid::Acting {
        state: peer.state(),
        input: &corvid::Input::new(pong::action::SETS),
        time: corvid::Time {
            tick: peer.tick(),
            ..corvid::Time::default()
        },
        seat: peer.seat(),
    });
    peer.submit(action)?;

    let mut arrived: Vec<Vec<u8>> = Vec::new();
    socket.poll(&mut |_, delivery| {
        if let Delivery::Datagram(bytes) = delivery {
            arrived.push(bytes.to_vec());
        }
    });
    for bytes in &arrived {
        // A packet that will not decode is dropped rather than fatal: an open
        // port carries whatever is sent to it.
        let Ok(datagram) = corvid_wire::decode::<Datagram<Move>>(bytes) else {
            continue;
        };
        peer.receive(&datagram)?;
    }

    let outgoing = corvid_wire::encode(&peer.outgoing())?;
    // A send that fails is a peer that is not reachable, which is predicted
    // through rather than reported.
    let _unreachable = socket.send_datagram(other, &outgoing);
    peer.advance(&mut corvid::Discard::new())?;
    Ok(())
}

/// Two peers play a session across two sockets and reach the same digest.
///
/// The peers take turns here rather than running on threads, and that is worth
/// being exact about: the *transport* is real and the *scheduling* is a loop.
/// What this test is for is the wire — that a datagram this workspace builds
/// survives a round trip through the operating system and is understood at the
/// other end — and `tests/linked.rs` is where two runtimes race each other on
/// their own clocks.
#[test]
fn two_peers_play_over_loopback() -> Fallible {
    let (here_socket, there_socket) = met()?;

    let mut here = Peer::<Table>::new(Session::new(opening())?, PlayerId(0), Budget::DEFAULT);
    let mut there = Peer::<Table>::new(Session::new(opening())?, PlayerId(1), Budget::DEFAULT);

    for _ in 0..TICKS {
        step(&mut here, &here_socket, PeerId(1))?;
        step(&mut there, &there_socket, PeerId(0))?;
        // Loopback is fast but not instant, and a peer that polled a
        // microsecond after the other sent would find nothing and predict. The
        // pause is what keeps this test about the wire rather than about how
        // quickly this machine schedules two loops.
        thread::sleep(Duration::from_millis(1));
    }

    // The line every seat has really spoken for on both sides. Above it each
    // peer has predicted the other, and two predictions disagreeing is what
    // prediction is.
    let line = here
        .frontier
        .agreed()
        .min(there.frontier.agreed())
        .min(here.tick())
        .min(there.tick());
    assert!(
        line.0 >= TICKS / 2,
        "only {} of {TICKS} ticks were confirmed on both sides",
        line.0,
    );

    for at in 0..=line.0 {
        assert_eq!(
            here.session.marks.get(Tick(at)),
            there.session.marks.get(Tick(at)),
            "the two peers disagree at tick {at}, over a real socket",
        );
    }

    // And they are playing pong rather than sitting still: the ball has been
    // served, hit, and is somewhere it could only have got to by being played.
    let table = here.state();
    assert!(table.now.0 >= line.0);
    assert!(
        table.scores[0] > 0 || table.scores[1] > 0 || !table.ball.velocity.x().is_zero(),
        "nothing happened in {TICKS} ticks: {table:?}",
    );
    // And the two peers' states agree at the line, which is the same claim the
    // trace comparison above makes, stated about the states themselves.
    assert_eq!(
        here.session.marks.get(line),
        Some(mark_of(&here.restore(line)?.1)),
        "this peer's own trace does not match the state it can restore",
    );
    Ok(())
}

/// A peer that hears nothing from a socket nobody is on predicts, stalls, and
/// does not fail.
///
/// The other half of the same claim `tests/session.rs` makes about a cut link,
/// made against a real socket: there is no opponent at that address at all.
#[test]
fn a_socket_with_nobody_on_it_stalls_rather_than_failing() -> Fallible {
    let socket = UdpNet::bind(("127.0.0.1", 0), PeerId(0))?;
    socket.connect(PeerId(1), "127.0.0.1:9")?;

    let mut alone = Peer::<Table>::new(Session::new(opening())?, PlayerId(0), Budget::DEFAULT);
    for _ in 0..100 {
        step(&mut alone, &socket, PeerId(1))?;
    }

    // It simulated as far as its budget allows past the tick every seat has
    // confirmed, and then waited — rather than playing on into a future nobody
    // agreed to, and rather than reporting an error.
    assert!(
        alone.tick().0 <= u64::from(Budget::DEFAULT.ahead) + u64::from(Budget::DEFAULT.delay) + 1,
        "a peer with nobody to play against reached tick {}",
        alone.tick().0,
    );
    Ok(())
}
