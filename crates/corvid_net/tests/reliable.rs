//! The reliability layer, with packets dropped on purpose.
//!
//! A loopback socket loses nothing, so a test that only drove
//! [`UdpNet`](corvid_net::udp::UdpNet) would never once exercise the code that
//! makes a reliable channel reliable. This drives the two halves directly and
//! decides for itself which pieces arrive.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "every test here returns a `Result` so that a failure reaches for `?` rather than unwrapping, and asserts as well — a failed assertion in a test is a failed test, which is what a test is for"
)]

use std::time::{Duration, Instant};

use corvid_net::udp::reliable::{FRAGMENT, IN_FLIGHT, Piece, RETRY, Receiver, Sender};

/// Whatever the test needs to say went wrong.
type Fallible = Result<(), Box<dyn std::error::Error>>;

/// A clock that only moves when a test says so.
fn later(from: Instant, by: Duration) -> Instant {
    from.checked_add(by).unwrap_or(from)
}

#[test]
fn a_frame_that_arrives_is_delivered_once() -> Fallible {
    let now = Instant::now();
    let (mut sender, mut receiver) = (Sender::default(), Receiver::default());

    let pieces = sender.send(b"hello", now)?;
    assert_eq!(pieces.len(), 1);

    let mut delivered = Vec::new();
    for piece in pieces {
        delivered.extend(receiver.accept(piece));
    }
    assert_eq!(delivered, [b"hello".to_vec()]);
    assert_eq!(receiver.expected(), 1);

    // And the sender stops holding it once the acknowledgement comes back.
    assert_eq!(sender.waiting(), 1);
    sender.acknowledged(receiver.expected());
    assert_eq!(sender.waiting(), 0);
    Ok(())
}

#[test]
fn a_lost_piece_is_sent_again_and_nothing_is_delivered_twice() -> Fallible {
    let start = Instant::now();
    let (mut sender, mut receiver) = (Sender::default(), Receiver::default());

    // Three frames, and the middle one never makes it.
    let mut sent = Vec::new();
    for frame in [b"one".as_slice(), b"two".as_slice(), b"three".as_slice()] {
        sent.extend(sender.send(frame, start)?);
    }
    assert_eq!(sent.len(), 3);

    let mut delivered = Vec::new();
    for piece in &sent {
        if piece.sequence == 1 {
            // Dropped on purpose.
            continue;
        }
        delivered.extend(receiver.accept(piece.clone()));
    }

    // The first arrived; the third is *held*, because a reliable channel is an
    // ordered one and the frame in front of it has not been seen.
    assert_eq!(delivered, [b"one".to_vec()]);
    assert_eq!(receiver.expected(), 1);
    assert_eq!(receiver.held(), 1);

    // The far end acknowledges what it has, which is one frame.
    sender.acknowledged(receiver.expected());
    assert_eq!(
        sender.waiting(),
        2,
        "the two it has not heard about are kept"
    );

    // Nothing is due yet.
    assert!(sender.due(start).is_empty());

    // And after the retry interval, the two unacknowledged pieces go again.
    let after = later(start, RETRY);
    let again = sender.due(after);
    assert_eq!(again.len(), 2);

    let mut recovered = Vec::new();
    for piece in again {
        recovered.extend(receiver.accept(piece));
    }
    // Both frames come out, in the order they were sent, and the one that
    // arrived twice is not delivered twice.
    assert_eq!(recovered, [b"two".to_vec(), b"three".to_vec()]);
    assert_eq!(receiver.expected(), 3);

    sender.acknowledged(receiver.expected());
    assert_eq!(sender.waiting(), 0);
    Ok(())
}

#[test]
fn a_duplicate_is_dropped() -> Fallible {
    let now = Instant::now();
    let (mut sender, mut receiver) = (Sender::default(), Receiver::default());
    let pieces = sender.send(b"once", now)?;

    let first: Vec<Vec<u8>> = pieces
        .iter()
        .flat_map(|piece| receiver.accept(piece.clone()))
        .collect();
    let again: Vec<Vec<u8>> = pieces
        .iter()
        .flat_map(|piece| receiver.accept(piece.clone()))
        .collect();

    assert_eq!(first, [b"once".to_vec()]);
    assert!(
        again.is_empty(),
        "a retransmission was delivered a second time"
    );
    Ok(())
}

#[test]
fn pieces_that_arrive_backwards_are_delivered_forwards() -> Fallible {
    let now = Instant::now();
    let (mut sender, mut receiver) = (Sender::default(), Receiver::default());
    let mut sent = Vec::new();
    for frame in [b"a".as_slice(), b"b".as_slice(), b"c".as_slice()] {
        sent.extend(sender.send(frame, now)?);
    }

    let mut delivered = Vec::new();
    for piece in sent.into_iter().rev() {
        delivered.extend(receiver.accept(piece));
    }
    assert_eq!(
        delivered,
        [b"a".to_vec(), b"b".to_vec(), b"c".to_vec()],
        "reordered pieces were not put back in order",
    );
    Ok(())
}

#[test]
fn a_frame_larger_than_a_packet_is_split_and_put_back_together() -> Fallible {
    let now = Instant::now();
    let (mut sender, mut receiver) = (Sender::default(), Receiver::default());

    // Two and a half fragments' worth, so the last piece is a short one.
    let big: Vec<u8> = (0..FRAGMENT * 5 / 2)
        .map(|at| u8::try_from(at % 251).unwrap_or(0))
        .collect();
    let pieces = sender.send(&big, now)?;
    assert_eq!(pieces.len(), 3);
    assert!(
        pieces[..2].iter().all(|piece| piece.more),
        "every piece but the last says another follows",
    );
    assert!(!pieces[2].more);

    let mut delivered = Vec::new();
    for piece in pieces {
        delivered.extend(receiver.accept(piece));
    }
    assert_eq!(delivered, [big], "the frame did not survive being split");
    Ok(())
}

#[test]
fn an_empty_frame_is_still_a_frame() -> Fallible {
    let now = Instant::now();
    let (mut sender, mut receiver) = (Sender::default(), Receiver::default());
    let pieces = sender.send(b"", now)?;
    assert_eq!(pieces.len(), 1);

    let delivered: Vec<Vec<u8>> = pieces
        .into_iter()
        .flat_map(|piece| receiver.accept(piece))
        .collect();
    assert_eq!(delivered, [Vec::<u8>::new()]);
    Ok(())
}

#[test]
fn a_sender_whose_far_end_has_stopped_answering_refuses_rather_than_grows() -> Fallible {
    let now = Instant::now();
    let mut sender = Sender::default();
    for _ in 0..IN_FLIGHT {
        sender.send(b"x", now)?;
    }
    assert_eq!(sender.waiting(), IN_FLIGHT);

    let refused = sender.send(b"one too many", now);
    assert!(
        refused.is_err(),
        "a queue with no acknowledgements coming back grew past its limit, which \
         is a memory allocation another machine decides the size of",
    );
    Ok(())
}

#[test]
fn a_piece_before_the_window_is_ignored_however_old() -> Fallible {
    let now = Instant::now();
    let (mut sender, mut receiver) = (Sender::default(), Receiver::default());
    for _ in 0..4 {
        for piece in sender.send(b"frame", now)? {
            drop(receiver.accept(piece));
        }
    }
    assert_eq!(receiver.expected(), 4);

    // A piece from the far past, which is what a very late retransmission is.
    let stale = Piece {
        sequence: 0,
        more: false,
        bytes: b"frame".to_vec(),
    };
    assert!(receiver.accept(stale).is_empty());
    assert_eq!(receiver.expected(), 4, "and the window did not move");
    Ok(())
}
