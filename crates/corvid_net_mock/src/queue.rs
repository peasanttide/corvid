//! The due-ordered delivery queue.

use core::{cmp::Ordering, time::Duration};
use std::{cmp::Reverse, collections::BinaryHeap};

use corvid_net::{Channel, Link};

/// What is waiting, and what to do when it comes due.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Wait {
    /// One unreliable datagram, already drawn against its link's schedule.
    Datagram { link: Link, bytes: Vec<u8> },
    /// One delivery attempt at the head of a reliable pipe. Whether it is
    /// going to succeed was drawn when the attempt was armed, so the pop that
    /// finds it lost only has to reschedule.
    Attempt {
        link: Link,
        channel: Channel,
        lost: bool,
    },
}

impl Wait {
    /// Which link this is travelling.
    pub(crate) const fn link(&self) -> Link {
        match *self {
            Self::Datagram { link, .. } | Self::Attempt { link, .. } => link,
        }
    }
}

/// One queued delivery, ordered by when it is due and then by when it was
/// queued.
///
/// The second half is what keeps two packets due at the same instant in send
/// order rather than in whatever order a container happened to hold them.
// `PartialEq` and `Eq` are written rather than derived, over the same two
// fields `Ord` compares. `Ord` requires that `cmp` answers `Equal` exactly when
// `eq` answers true, and a derive over `wait` as well would break that the day
// anything dedups or compares two entries -- `sequence` is unique today, which
// is the only reason a derive is not already wrong.
#[derive(Clone, Debug)]
pub(crate) struct Pending {
    /// The instant this is delivered at.
    pub(crate) due: Duration,
    /// Which item this was, counted across the whole network.
    pub(crate) sequence: u64,
    /// What to do then.
    pub(crate) wait: Wait,
}

impl PartialEq for Pending {
    fn eq(&self, other: &Self) -> bool {
        self.due == other.due && self.sequence == other.sequence
    }
}

impl Eq for Pending {}

impl Ord for Pending {
    fn cmp(&self, other: &Self) -> Ordering {
        self.due
            .cmp(&other.due)
            .then_with(|| self.sequence.cmp(&other.sequence))
    }
}

impl PartialOrd for Pending {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Everything in flight, keyed by `(due, sequence)`.
#[derive(Debug, Default)]
pub(crate) struct Queue {
    /// Reversed, because `BinaryHeap` is a max-heap and the next delivery is
    /// the smallest key.
    heap: BinaryHeap<Reverse<Pending>>,
    /// Bumped by every push, so the tie-break is never a hash map's iteration
    /// order.
    sequence: u64,
}

impl Queue {
    /// Queues one item.
    pub(crate) fn push(&mut self, due: Duration, wait: Wait) {
        self.heap.push(Reverse(Pending {
            due,
            sequence: self.sequence,
            wait,
        }));
        self.sequence += 1;
    }

    /// The next item due at or before `now`, if there is one.
    pub(crate) fn pop_due(&mut self, now: Duration) -> Option<Pending> {
        let ready = self
            .heap
            .peek()
            .is_some_and(|Reverse(next)| next.due <= now);
        if ready {
            self.heap.pop().map(|Reverse(pending)| pending)
        } else {
            None
        }
    }

    /// Drops everything `keep` refuses.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&Wait) -> bool) {
        self.heap.retain(|Reverse(pending)| keep(&pending.wait));
    }
}
