//! Two real sockets, on loopback.
//!
//! Everything here goes through the operating system: `bind`, `send_to`,
//! `recv_from`. What it proves that `MockNet` cannot is that the framing is a
//! framing -- that two processes speaking this protocol understand each other --
//! and what it cannot prove is anything about loss, because loopback does not
//! lose. `tests/reliable.rs` is where packets are dropped on purpose.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "every test here returns a `Result` so that a failure reaches for `?` rather than unwrapping, and asserts as well -- a failed assertion in a test is a failed test, which is what a test is for"
)]

use std::{
    thread,
    time::{Duration, Instant},
};

use corvid_net::{Channel, Delivery, PeerId, Transport};
use corvid_net_udp::UdpNet;

/// Whatever the test needs to say went wrong.
type Fallible = Result<(), Box<dyn std::error::Error>>;

/// How long a test waits for something that should take microseconds.
///
/// Generous, because a build machine under load is a build machine under load,
/// and a test that fails there rather than here is a test nobody trusts.
const PATIENCE: Duration = Duration::from_secs(5);

/// Two sockets on loopback, each told where the other is.
fn pair() -> Fallible2 {
    let here = UdpNet::bind(("127.0.0.1", 0), PeerId(0))?;
    let there = UdpNet::bind(("127.0.0.1", 0), PeerId(1))?;
    here.connect(PeerId(1), there.local()?)?;
    there.connect(PeerId(0), here.local()?)?;
    Ok((here, there))
}

/// The pair, or whatever the operating system said.
type Fallible2 = Result<(UdpNet, UdpNet), Box<dyn std::error::Error>>;

/// What one round of polling hands the test.
enum Round<'a> {
    /// Something arrived. The `bool` is whether it arrived at the second
    /// socket, which is how a test tells the two ends apart.
    Heard(PeerId, Delivery<'a>, bool),
    /// Both ends have been polled. Answering `true` here ends the wait.
    Done,
}

/// Polls both ends until the test says it has what it wanted, or gives up.
///
/// Everything this backend does happens in `poll` -- reading, acknowledging,
/// retransmitting, greeting -- so a test that wants something to happen polls.
///
/// One closure rather than a sink and a predicate, because the two would both
/// want the same local: the borrow checker is right that a `&mut` collector and
/// a `&` reader of the same `Vec` cannot both be live, and the answer is to ask
/// the same closure both questions.
fn until(here: &UdpNet, there: &UdpNet, mut step: impl FnMut(Round<'_>) -> bool) -> bool {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        here.poll(&mut |from, delivery| {
            let _collected = step(Round::Heard(from, delivery, false));
        });
        there.poll(&mut |from, delivery| {
            let _collected = step(Round::Heard(from, delivery, true));
        });
        if step(Round::Done) {
            return true;
        }
        thread::sleep(Duration::from_millis(2));
    }
    false
}

/// Polls both ends until each has the other in its published roster.
fn joined(here: &UdpNet, there: &UdpNet) -> bool {
    until(here, there, |round| {
        matches!(round, Round::Done) && !here.peers().is_empty() && !there.peers().is_empty()
    })
}

#[test]
fn two_sockets_find_each_other() -> Fallible {
    let (here, there) = pair()?;

    let mut greetings = 0_u32;
    let met = until(&here, &there, |round| match round {
        Round::Heard(_, Delivery::Joined, _) => {
            greetings += 1;
            false
        }
        Round::Done => here.peers().len() == 1 && there.peers().len() == 1,
        Round::Heard(..) => false,
    });

    assert!(met, "two sockets on loopback did not find each other");
    assert!(greetings >= 2, "only {greetings} joins were reported");
    assert!(here.peers().contains(PeerId(1)));
    assert!(there.peers().contains(PeerId(0)));
    Ok(())
}

#[test]
fn a_datagram_crosses_a_real_socket() -> Fallible {
    let (here, there) = pair()?;
    assert!(joined(&here, &there), "the two sockets did not meet");

    here.send_datagram(PeerId(1), b"tick 41")?;

    let mut heard: Vec<(PeerId, Vec<u8>)> = Vec::new();
    let arrived = until(&here, &there, |round| match round {
        Round::Heard(from, Delivery::Datagram(bytes), true) => {
            heard.push((from, bytes.to_vec()));
            false
        }
        Round::Done => !heard.is_empty(),
        Round::Heard(..) => false,
    });

    assert!(arrived, "a datagram sent over loopback never arrived");
    assert_eq!(heard.first(), Some(&(PeerId(0), b"tick 41".to_vec())));
    Ok(())
}

#[test]
fn a_datagram_past_the_limit_is_refused_rather_than_sent() -> Fallible {
    let (here, there) = pair()?;
    assert!(joined(&here, &there), "the two sockets did not meet");

    let big = vec![0_u8; corvid_net::DATAGRAM_LIMIT + 1];
    let refused = here.send_datagram(PeerId(1), &big);
    assert!(
        matches!(refused, Err(corvid_net::SendError::TooLarge { .. })),
        "{refused:?}",
    );
    Ok(())
}

#[test]
fn a_peer_nobody_has_heard_from_is_not_reachable() -> Fallible {
    let here = UdpNet::bind(("127.0.0.1", 0), PeerId(0))?;
    // An address on the discard port, which nothing is listening on.
    here.connect(PeerId(1), "127.0.0.1:9")?;

    let refused = here.send_datagram(PeerId(1), b"anyone there");
    assert!(
        matches!(refused, Err(corvid_net::SendError::Unknown(PeerId(1)))),
        "a peer that has never answered was treated as reachable: {refused:?}",
    );
    assert!(here.peers().is_empty());
    Ok(())
}

#[test]
fn a_reliable_frame_crosses_in_order_and_whole() -> Fallible {
    let (here, there) = pair()?;
    assert!(joined(&here, &there), "the two sockets did not meet");

    // Twenty small frames and one large one, which is more than a packet holds
    // and is therefore split and put back together at the far end.
    let big: Vec<u8> = (0..8_000_u32)
        .map(|at| u8::try_from(at % 251).unwrap_or(0))
        .collect();
    for index in 0..20_u8 {
        here.send_stream(PeerId(1), Channel::Control, &[index])?;
    }
    here.send_stream(PeerId(1), Channel::Transfer, &big)?;

    let mut control: Vec<Vec<u8>> = Vec::new();
    let mut transfer: Vec<Vec<u8>> = Vec::new();
    let all = until(&here, &there, |round| match round {
        Round::Heard(_, Delivery::Stream { channel, bytes }, true) => {
            match channel {
                Channel::Control => control.push(bytes.to_vec()),
                Channel::Transfer => transfer.push(bytes.to_vec()),
                _ => {}
            }
            false
        }
        Round::Done => control.len() == 20 && transfer.len() == 1,
        Round::Heard(..) => false,
    });

    assert!(
        all,
        "the reliable channels delivered {} control frames and {} transfers",
        control.len(),
        transfer.len(),
    );
    assert_eq!(
        control,
        (0..20_u8).map(|index| vec![index]).collect::<Vec<_>>(),
        "the control channel delivered its frames out of order",
    );
    assert_eq!(
        transfer.first(),
        Some(&big),
        "the large frame did not survive"
    );
    Ok(())
}

#[test]
fn a_channel_does_not_hold_up_another() -> Fallible {
    // Ordering is *within* a channel and not across them, which is what
    // `Channel`'s own documentation promises and is the reason a state transfer
    // does not delay a chat line. Loopback delivers in order anyway, so what
    // this really pins is that the two channels have their own sequence
    // numbers: a receiver that shared one would hold the chat line behind the
    // transfer's first piece.
    let (here, there) = pair()?;
    assert!(joined(&here, &there), "the two sockets did not meet");

    let big: Vec<u8> = vec![7; 4_000];
    here.send_stream(PeerId(1), Channel::Transfer, &big)?;
    here.send_stream(PeerId(1), Channel::Chat, b"hello")?;

    let mut chat: Option<Vec<u8>> = None;
    let mut transfer: Option<Vec<u8>> = None;
    let both = until(&here, &there, |round| match round {
        Round::Heard(_, Delivery::Stream { channel, bytes }, true) => {
            match channel {
                Channel::Chat => chat = Some(bytes.to_vec()),
                Channel::Transfer => transfer = Some(bytes.to_vec()),
                _ => {}
            }
            false
        }
        Round::Done => chat.is_some() && transfer.is_some(),
        Round::Heard(..) => false,
    });

    assert!(
        both,
        "chat: {:?}, transfer: {:?}",
        chat.is_some(),
        transfer.is_some()
    );
    assert_eq!(chat.as_deref(), Some(b"hello".as_slice()));
    assert_eq!(transfer, Some(big));
    Ok(())
}

#[test]
fn a_socket_that_goes_away_says_goodbye() -> Fallible {
    let (here, there) = pair()?;
    assert!(joined(&here, &there), "the two sockets did not meet");

    // What a process exiting looks like from the other machine. Without this a
    // peer waits out the timeout, and a lockstep session waits with it.
    drop(here);

    let mut lost = None;
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline && lost.is_none() {
        there.poll(&mut |from, delivery| {
            if let Delivery::Lost { because } = delivery {
                lost = Some((from, because));
            }
        });
        thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(lost, Some((PeerId(0), corvid_net::Lost::Closed)));
    assert!(
        there.peers().is_empty(),
        "a peer that said goodbye is still in the roster",
    );
    Ok(())
}

#[test]
fn a_stray_packet_is_dropped_rather_than_parsed() -> Fallible {
    let (here, there) = pair()?;
    assert!(joined(&here, &there), "the two sockets did not meet");

    // Anything can reach an open port. None of this is this protocol, and the
    // socket carries on afterwards -- which is the assertion.
    let stranger = std::net::UdpSocket::bind(("127.0.0.1", 0))?;
    let target = there.local()?;
    for noise in [
        b"".as_slice(),
        b"hello?".as_slice(),
        b"CVDN".as_slice(),
        &[b'C', b'V', b'D', b'N', 99, 2, 0, 0],
        &[0xff; 700],
    ] {
        let _sent = stranger.send_to(noise, target)?;
    }

    here.send_datagram(PeerId(1), b"still here")?;
    let mut heard: Vec<Vec<u8>> = Vec::new();
    let arrived = until(&here, &there, |round| match round {
        Round::Heard(_, Delivery::Datagram(bytes), true) => {
            heard.push(bytes.to_vec());
            false
        }
        Round::Done => !heard.is_empty(),
        Round::Heard(..) => false,
    });

    assert!(
        arrived,
        "a socket stopped working after being sent nonsense"
    );
    assert_eq!(heard, [b"still here".to_vec()]);
    Ok(())
}
