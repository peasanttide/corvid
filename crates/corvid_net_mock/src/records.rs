//! The three records the network keeps per peer, per channel and per link.
//!
//! None of them is public. They are here rather than beside the engine that
//! mutates them because what they *are* is separable from what is done to
//! them, and the engine is the longer read of the two.

use std::collections::VecDeque;
use std::time::Duration;

use corvid_net::{Channel, Delivery, Lost, PeerId};

use crate::schedule::Schedule;

/// What is sitting in one peer's inbox, owned until it is polled.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Arrival {
    Datagram(Vec<u8>),
    Stream { channel: Channel, bytes: Vec<u8> },
    Joined,
    Lost(Lost),
}

impl Arrival {
    /// Whether the inbox bound is allowed to drop this.
    ///
    /// A datagram, and nothing else. A join or a loss is what tells a peer who
    /// it is talking to, and a stream frame was promised to arrive in order --
    /// dropping one leaves a hole in a channel whose whole contract is that
    /// there are none. Those are held back at the pipe instead, which is where
    /// a reliable transport puts a receiver that has stopped reading.
    pub(crate) const fn is_droppable(&self) -> bool {
        matches!(self, Self::Datagram(_))
    }

    /// Whether this counts against the inbox bound.
    ///
    /// A datagram or a stream frame -- the things a sender chose to send. A
    /// join and a loss are the network telling a peer who it is talking to,
    /// and they arrive whether anyone sent anything or not, so counting them
    /// would make the allowance depend on how many peers are in the session.
    pub(crate) const fn is_traffic(&self) -> bool {
        matches!(self, Self::Datagram(_) | Self::Stream { .. })
    }

    /// The borrowed view a [`Transport::poll`] sink is handed.
    pub(crate) fn delivery(&self) -> Delivery<'_> {
        match self {
            Self::Datagram(bytes) => Delivery::Datagram(bytes),
            Self::Stream { channel, bytes } => Delivery::Stream {
                channel: *channel,
                bytes,
            },
            Self::Joined => Delivery::Joined,
            Self::Lost(because) => Delivery::Lost { because: *because },
        }
    }
}

/// One peer's inbox: what is waiting, and how much of it is traffic.
///
/// The traffic count is kept rather than computed, because the bound is
/// checked on every arrival and a scan would make that quadratic in exactly
/// the case the bound exists for -- a peer that has stopped polling.
#[derive(Clone, Debug, Default)]
pub(crate) struct Inbox {
    /// Oldest first.
    pub(crate) waiting: VecDeque<(PeerId, Arrival)>,
    /// How many of `waiting` are datagrams or stream frames.
    ///
    /// Only this is bounded. Counting joins and losses too would mean a
    /// network with more peers than the bound opened every inbox already over
    /// it, before a byte was sent -- every datagram evicted the instant it
    /// landed and every reliable frame withheld for ever.
    pub(crate) traffic: usize,
}

impl Inbox {
    /// The inbox a peer opens with: a join from everyone else, and no traffic.
    pub(crate) fn opening(joins: impl IntoIterator<Item = PeerId>) -> Self {
        Self {
            waiting: joins.into_iter().map(|it| (it, Arrival::Joined)).collect(),
            traffic: 0,
        }
    }

    /// Adds one arrival, keeping the count level with the queue.
    pub(crate) fn push(&mut self, from: PeerId, arrival: Arrival) {
        if arrival.is_traffic() {
            self.traffic += 1;
        }
        self.waiting.push_back((from, arrival));
    }

    /// Removes the arrival at `at`, keeping the count level with the queue.
    pub(crate) fn remove(&mut self, at: usize) -> Option<(PeerId, Arrival)> {
        let taken = self.waiting.remove(at)?;
        if taken.1.is_traffic() {
            self.traffic -= 1;
        }
        Some(taken)
    }
}

/// One reliable channel in one direction.
///
/// Only the frame at the head is ever in flight, so a retransmit holds
/// everything behind it -- which is what makes a state transfer over a bad link
/// take a visible amount of time rather than an averaged one.
#[derive(Debug, Default)]
pub(crate) struct Pipe {
    /// Written but not yet delivered, oldest first.
    pub(crate) frames: VecDeque<Vec<u8>>,
    /// Whether an attempt at the head is already queued.
    pub(crate) armed: bool,
}

/// One direction of one link, as the network holds it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LinkState {
    /// The curve it follows.
    pub(crate) schedule: Schedule,
    /// How many scheduling decisions this link's draw stream has served.
    pub(crate) draws: u64,
    /// The due of the last datagram scheduled here -- the neighbour a reordered
    /// one is moved across.
    pub(crate) last_due: Duration,
}

impl Default for LinkState {
    fn default() -> Self {
        Self {
            schedule: Schedule::PERFECT,
            draws: 0,
            last_due: Duration::ZERO,
        }
    }
}
