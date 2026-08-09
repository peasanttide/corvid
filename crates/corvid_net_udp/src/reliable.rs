//! Reliability and ordering over something that has neither.
//!
//! One of these per `(peer, channel)`. It is deliberately free of sockets and
//! of clocks -- time arrives as an argument -- because that is what lets the
//! interesting half be tested by dropping packets on purpose rather than by
//! hoping a loopback link loses one. `tests/reliable.rs` is that test.
//!
//! # What this is, and what it is not
//!
//! It is a stop-and-go window: frames go out with consecutive sequence numbers,
//! the far end acknowledges the newest run it has *complete*, and anything
//! unacknowledged is sent again after [`RETRY`]. That gives the two properties
//! [`Channel`](corvid_net::Channel) promises -- every frame arrives, and they arrive
//! in the order they were sent -- and nothing else.
//!
//! It is not a congestion controller. There is no window sizing, no round-trip
//! estimate and no back-off: a fixed retry interval and a cap on how much may
//! be in flight at once. On a local network, which is what this backend is for,
//! that is enough; on a path that is genuinely congested it would be one of the
//! flows making it worse, and the honest answer there is QUIC rather than a
//! better version of this.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// How long an unacknowledged frame waits before it goes again.
///
/// Fixed, rather than derived from a measured round trip. Fifty milliseconds is
/// long enough that a local network never retransmits unnecessarily and short
/// enough that a lost frame on one does not stall a channel for a noticeable
/// time. A path with a longer round trip than this retransmits frames that were
/// merely late, which costs bandwidth and never costs correctness -- a duplicate
/// is dropped by sequence number at the far end.
pub const RETRY: Duration = Duration::from_millis(50);

/// How many frames may be unacknowledged at once.
///
/// Past it, [`send`](Sender::send) refuses. A sender with an unbounded queue is
/// a memory allocation controlled by whether somebody else's machine is
/// answering, which is the shape of a denial of service rather than of a
/// transport.
pub const IN_FLIGHT: usize = 256;

/// How much of one frame goes in one packet.
///
/// A frame larger than this is split, and the pieces are reassembled in order
/// at the far end -- which is free, because the pieces arrive in order by
/// construction.
pub const FRAGMENT: usize = 1_000;

/// The most bytes one reassembled frame may hold.
///
/// A remote peer decides how many fragments to send and this is what stops that
/// from being a request for as much memory as it likes. Sixteen mebibytes is
/// far past any state transfer this workspace produces and far short of a
/// machine's memory.
pub const FRAME_LIMIT: usize = 16 << 20;

/// One packet's worth of a channel's traffic, before it has a header on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    /// Which packet in this channel's sequence.
    pub sequence: u32,
    /// Whether another piece of the same frame follows.
    pub more: bool,
    /// The bytes.
    pub bytes: Vec<u8>,
}

/// The sending half of one channel.
#[derive(Debug, Default)]
pub struct Sender {
    /// The sequence number the next piece goes out under.
    next: u32,
    /// Every piece that has not been acknowledged, oldest first, with when it
    /// was last sent.
    unacked: Vec<(Piece, Instant)>,
}

impl Sender {
    /// Splits a frame into pieces and queues them.
    ///
    /// Answers what to put on the wire now. Nothing is retained by the caller:
    /// the pieces stay here until they are acknowledged, so
    /// [`due`](Self::due) can offer them again.
    ///
    /// # Errors
    ///
    /// [`TooMuch`] when the queue is full, which is a peer that has stopped
    /// acknowledging.
    pub fn send(&mut self, bytes: &[u8], now: Instant) -> Result<Vec<Piece>, TooMuch> {
        // An empty frame is a frame: a caller that sent no bytes meant to send
        // no bytes, and dropping it would make `send_stream` silently lossy for
        // exactly one input.
        let chunks = bytes.chunks(FRAGMENT).count().max(1);
        if self.unacked.len().saturating_add(chunks) > IN_FLIGHT {
            return Err(TooMuch {
                waiting: self.unacked.len(),
                limit: IN_FLIGHT,
            });
        }

        let mut pieces = Vec::with_capacity(chunks);
        let mut chunked = bytes.chunks(FRAGMENT);
        // The empty frame is why this is a loop with the chunk carried rather
        // than a `for`: `[].chunks(n)` yields nothing at all, and a frame of no
        // bytes still has to become one piece.
        let mut chunk: &[u8] = chunked.next().unwrap_or(&[]);
        loop {
            let following = chunked.next();
            let piece = Piece {
                sequence: self.next,
                more: following.is_some(),
                bytes: chunk.to_vec(),
            };
            self.next = self.next.wrapping_add(1);
            self.unacked.push((piece.clone(), now));
            pieces.push(piece);
            match following {
                Some(next) => chunk = next,
                None => break,
            }
        }
        Ok(pieces)
    }

    /// Everything that has waited longer than [`RETRY`], marked as sent again.
    pub fn due(&mut self, now: Instant) -> Vec<Piece> {
        let mut again = Vec::new();
        for (piece, sent) in &mut self.unacked {
            if now.duration_since(*sent) >= RETRY {
                *sent = now;
                again.push(piece.clone());
            }
        }
        again
    }

    /// Drops everything the far end says it has.
    ///
    /// `through` is the sequence number the receiver wants next, so everything
    /// below it has arrived. A cumulative acknowledgement rather than a
    /// selective one: one number covers a run of frames, and a lost
    /// acknowledgement costs a retransmission rather than a stall.
    pub fn acknowledged(&mut self, through: u32) {
        // Wrapping order rather than `>=`: a sequence number is a `u32` that
        // wraps, and a channel that carried four billion frames would otherwise
        // drop everything the moment it did. A piece is kept when it is at or
        // after `through` going forwards, which is what a difference below half
        // the range means.
        self.unacked
            .retain(|(piece, _)| piece.sequence.wrapping_sub(through) < u32::MAX / 2);
    }

    /// How many pieces are waiting to be acknowledged.
    #[must_use]
    pub const fn waiting(&self) -> usize {
        self.unacked.len()
    }
}

/// The receiving half of one channel.
#[derive(Debug, Default)]
pub struct Receiver {
    /// The sequence number wanted next. Everything below it has been released.
    expected: u32,
    /// Pieces that arrived early, by sequence number.
    held: BTreeMap<u32, Piece>,
    /// The pieces of the frame being reassembled, in order.
    partial: Vec<u8>,
}

impl Receiver {
    /// Folds in a piece and answers whatever whole frames that completed.
    ///
    /// A piece below [`expected`](Self::expected) is a duplicate -- a
    /// retransmission of something already released -- and is dropped. A piece
    /// above it is held until the gap is filled, which is the head-of-line
    /// blocking a reliable ordered channel *is*.
    pub fn accept(&mut self, piece: Piece) -> Vec<Vec<u8>> {
        if piece.sequence.wrapping_sub(self.expected) >= u32::MAX / 2 {
            // At or below what has already been released, in wrapping order.
            return Vec::new();
        }
        self.held.insert(piece.sequence, piece);

        let mut frames = Vec::new();
        while let Some(piece) = self.held.remove(&self.expected) {
            self.expected = self.expected.wrapping_add(1);
            if self.partial.len().saturating_add(piece.bytes.len()) > FRAME_LIMIT {
                // A frame this receiver will not assemble. What has been held
                // is dropped rather than grown: the alternative is letting a
                // remote peer decide how much memory this process uses.
                self.partial.clear();
                continue;
            }
            self.partial.extend_from_slice(&piece.bytes);
            if !piece.more {
                frames.push(std::mem::take(&mut self.partial));
            }
        }
        frames
    }

    /// The sequence number this receiver wants next, which is what it
    /// acknowledges.
    #[must_use]
    pub const fn expected(&self) -> u32 {
        self.expected
    }

    /// How many pieces are held waiting for a gap to be filled.
    #[must_use]
    pub fn held(&self) -> usize {
        self.held.len()
    }
}

/// A channel with too much already in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TooMuch {
    /// How many pieces are waiting.
    pub waiting: usize,
    /// How many may.
    pub limit: usize,
}

impl std::fmt::Display for TooMuch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} frames are already waiting to be acknowledged and the limit is {}",
            self.waiting, self.limit,
        )
    }
}

impl std::error::Error for TooMuch {}
