//! The two handles a caller holds: the network, and one peer's end of it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{fmt, mem};

use corvid_net::{
    Channel, DATAGRAM_LIMIT, Delivery, Link, Lost, PeerId, PeerSet, SendError, Transport,
};
use corvid_signal::{Watch, channel};

use crate::engine::{Inner, Shared};
use crate::queue::Queue;
use crate::records::{Arrival, Inbox};
use crate::schedule::Schedule;
use crate::tally::Tally;

/// Peers linked in-process, with scriptable latency, jitter, loss and reorder.
///
/// Public API rather than a test helper: a netcode lab and a netcode test are
/// this with different assertions, and a downstream game builds its own netcode
/// tests out of it.
///
/// Nothing is delivered by a thread. The clock moves only in
/// [`advance`](Self::advance), so a test drives it a step at a time and a lab
/// drives it from its frame loop, and the two are the same code.
///
/// The reproducibility that buys is for **one driver**. A `MockNet` is `Sync`
/// and every method takes `&self`, so several threads may hold endpoints -- but
/// each send draws from its link's stream in the order the lock is acquired,
/// and two threads racing to send take it in whatever order the operating
/// system decides. Same seed, different schedule. Drive the network from one
/// thread and the schedule is a function of the seed alone; share it across
/// threads and it is a function of the seed and the scheduler.
///
/// ```
/// use core::time::Duration;
///
/// use corvid_net::{Delivery, PeerId, Transport as _};
/// use corvid_net_mock::{MockNet, Schedule};
///
/// let net = MockNet::new(2, 0x5eed);
/// net.all(Schedule::DOMESTIC);
///
/// let alice = net.endpoint(PeerId(1));
/// let bob = net.endpoint(PeerId(2));
///
/// alice.send_datagram(PeerId(2), b"tick 41")?;
///
/// // Forty milliseconds is the floor, so nothing has arrived yet.
/// net.advance(Duration::from_millis(20));
/// let mut heard = Vec::new();
/// bob.poll(&mut |from, what| {
///     if let Delivery::Datagram(bytes) = what {
///         heard.push((from, bytes.to_vec()));
///     }
/// });
/// assert!(heard.is_empty());
///
/// net.advance(Duration::from_millis(60));
/// bob.poll(&mut |from, what| {
///     if let Delivery::Datagram(bytes) = what {
///         heard.push((from, bytes.to_vec()));
///     }
/// });
/// assert_eq!(heard, [(PeerId(1), b"tick 41".to_vec())]);
/// # Ok::<(), corvid_net::SendError>(())
/// ```
#[derive(Clone, Debug)]
pub struct MockNet {
    shared: Arc<Shared>,
}

impl MockNet {
    /// A network with `peers` seats, every link [perfect](Schedule::PERFECT)
    /// and everyone already connected to everyone else.
    ///
    /// Every peer's inbox opens with a [`Delivery::Joined`] for every other
    /// peer, in peer order, so the rule that a join precedes everything else
    /// from that peer holds from the first poll.
    #[must_use]
    pub fn new(peers: u16, seed: u64) -> Self {
        // Seats are `PeerId(1)` upwards, because `PeerId(0)` is
        // [`PeerId::NONE`] and nobody is not a seat. Every vector here is
        // still indexed by a peer's own number, so nought is a slot that
        // exists and stays empty -- which is also what makes nobody unroutable
        // for free: an empty roster contains no one, and no one's roster
        // contains nobody, so `route` refuses either way.
        let count = usize::from(peers) + 1;
        let mut rosters: Vec<PeerSet> = vec![PeerSet::new(); count];
        let mut inboxes: Vec<Inbox> = vec![Inbox::default(); count];

        for me in 1..=peers {
            let seat = usize::from(me);
            rosters[seat] = (1..=peers).filter(|&it| it != me).map(PeerId).collect();
            inboxes[seat] = Inbox::opening((1..=peers).filter(|&it| it != me).map(PeerId));
        }

        let (emitters, watches) = rosters
            .iter()
            .map(|roster: &PeerSet| channel("peers", roster.clone()))
            .unzip();

        Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    now: Duration::ZERO,
                    seed,
                    links: BTreeMap::new(),
                    pipes: BTreeMap::new(),
                    queue: Queue::default(),
                    inboxes,
                    rosters,
                    tally: Tally::default(),
                }),
                emitters,
                watches,
            }),
        }
    }

    /// How many seats this network has.
    ///
    /// Seats are `PeerId(1)` up to and including `PeerId(peers())`. The
    /// vectors underneath carry a slot for nobody at nought, which this does
    /// not count.
    #[must_use]
    pub fn peers(&self) -> u16 {
        u16::try_from(self.shared.watches.len().saturating_sub(1)).unwrap_or(u16::MAX)
    }

    /// One peer's handle. Hand it to whatever is playing that seat; it
    /// implements [`Transport`].
    ///
    /// A peer this network does not have -- [`PeerId::NONE`] among them -- gets
    /// an endpoint that reaches nobody: its roster is empty and every send
    /// answers [`SendError::Unknown`].
    #[must_use]
    pub fn endpoint(&self, peer: PeerId) -> Endpoint {
        let peers = self
            .shared
            .watches
            .get(usize::from(peer.to_u16()))
            .cloned()
            .unwrap_or_else(|| channel("peers", PeerSet::new()).1);

        Endpoint {
            net: Arc::clone(&self.shared),
            me: peer,
            peers,
        }
    }

    /// Sets the curve one direction of one link follows.
    pub fn link(&self, from: PeerId, to: PeerId, schedule: Schedule) {
        let mut inner = self.shared.lock();
        inner.links.entry(Link::new(from, to)).or_default().schedule = schedule;
    }

    /// Sets every link at once, both directions of each.
    pub fn all(&self, schedule: Schedule) {
        let peers = self.peers();
        let mut inner = self.shared.lock();
        for from in 1..=peers {
            for to in 1..=peers {
                if from != to {
                    inner
                        .links
                        .entry(Link::new(PeerId(from), PeerId(to)))
                        .or_default()
                        .schedule = schedule;
                }
            }
        }
    }

    /// Moves time forward and delivers whatever is due.
    ///
    /// An advance of no time reaches no instant, so it delivers nothing and
    /// changes no tally -- including on a link with no latency at all, where
    /// everything is due the moment it is sent.
    pub fn advance(&self, dt: Duration) {
        if dt.is_zero() {
            return;
        }
        let mut inner = self.shared.lock();
        inner.now = inner.now.saturating_add(dt);
        inner.deliver();
    }

    /// Where the clock is.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.shared.lock().now
    }

    /// Severs a link. Both directions go, both sides are told, and everything
    /// that was in flight either way is forgotten.
    ///
    /// A link already severed is left alone, so a second cut delivers nothing.
    pub fn cut(&self, from: PeerId, to: PeerId, because: Lost) {
        {
            let mut inner = self.shared.lock();
            let forward = inner
                .rosters
                .get_mut(usize::from(from.to_u16()))
                .is_some_and(|roster| roster.remove(to));
            let back = inner
                .rosters
                .get_mut(usize::from(to.to_u16()))
                .is_some_and(|roster| roster.remove(from));
            if !forward && !back {
                return;
            }

            let lost = inner.discard(Link::new(from, to)) + inner.discard(Link::new(to, from));
            inner.tally.in_flight = inner.tally.in_flight.saturating_sub(lost);

            inner.post(from, to, Arrival::Lost(because));
            inner.post(to, from, Arrival::Lost(because));
        }

        self.shared.publish(from);
        self.shared.publish(to);
    }

    /// What has happened.
    #[must_use]
    pub fn tally(&self) -> Tally {
        self.shared.lock().tally
    }
}

/// The count at which one peer's inbox begins dropping its oldest datagram.
///
/// A threshold and not a hard capacity, which is the part worth reading twice.
/// A join and a loss are never dropped -- a peer that fell behind would
/// otherwise wake to traffic from someone it was never told about -- so an
/// inbox holds this many datagrams *plus* up to two connection events per
/// peer. Stream frames cannot push it over either, but for the opposite
/// reason: a full inbox stops them at the pipe rather than delivering them.
///
/// A thousand is far more than a peer polling once a tick ever accumulates --
/// at thirty ticks a second and one datagram per peer per tick, it is half a
/// minute of traffic from a peer that has stopped reading.
pub const INBOX: usize = 1_000;

/// How many frames one channel will hold before [`send_stream`] refuses.
///
/// [`send_stream`]: Transport::send_stream
///
/// A bound here rather than an unbounded queue, for the reason [`INBOX`] is
/// bounded and for one more that matters more. [`SendError::Backpressure`] is
/// part of the trait's contract, so a caller has to handle it -- and a caller
/// tested only against a `MockNet` that never returns one has handling that
/// first runs in production, against a real socket, on the day a peer stops
/// acknowledging. A test double that cannot produce an error the trait promises
/// is a test double that hides it.
///
/// Only the frame at the head of a channel is ever in flight, so this is a
/// backlog rather than a window: reaching it means the far end has not
/// acknowledged in a long time, which is the condition the variant names.
pub const QUEUED: usize = 256;

/// One peer's view of a [`MockNet`].
///
/// Cheap to clone, and every clone is the same seat.
#[derive(Clone)]
pub struct Endpoint {
    net: Arc<Shared>,
    me: PeerId,
    peers: Watch<PeerSet>,
}

// Written rather than derived, because [`Transport`] asks an implementation for
// "a name and its own bookkeeping, not the traffic through it" and a derive
// here reaches `Shared` and prints every peer's inbox -- payloads included --
// every roster and the whole delivery queue. On a lab with six hundred seats
// that is a multi-megabyte log line carrying the game's own traffic.
impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Endpoint")
            .field("me", &self.me)
            .field("peers", &self.peers.get().len())
            .finish_non_exhaustive()
    }
}

impl Endpoint {
    /// Which seat this is.
    #[must_use]
    pub const fn peer(&self) -> PeerId {
        self.me
    }
}

impl Transport for Endpoint {
    fn send_datagram(&self, to: PeerId, bytes: &[u8]) -> Result<(), SendError> {
        let mut inner = self.net.lock();
        let link = inner.route(self.me, to)?;
        if bytes.len() > DATAGRAM_LIMIT {
            return Err(SendError::TooLarge {
                bytes: bytes.len(),
                limit: DATAGRAM_LIMIT,
            });
        }
        inner.offer(link, bytes.to_vec());
        drop(inner);
        Ok(())
    }

    fn send_stream(&self, to: PeerId, channel: Channel, bytes: &[u8]) -> Result<(), SendError> {
        let mut inner = self.net.lock();
        let link = inner.route(self.me, to)?;
        let wrote = inner.write(link, channel, bytes);
        drop(inner);
        wrote
    }

    fn poll(&self, sink: &mut dyn FnMut(PeerId, Delivery<'_>)) {
        // Taken whole and under the lock, then handed out with the lock
        // released -- which is what lets a sink send, cut a link or advance the
        // clock from inside the loop.
        let arrivals = {
            let mut inner = self.net.lock();
            inner
                .inboxes
                .get_mut(usize::from(self.me.to_u16()))
                .map(mem::take)
                .unwrap_or_default()
        };

        for (from, arrival) in &arrivals.waiting {
            sink(*from, arrival.delivery());
        }
    }

    fn peers(&self) -> &Watch<PeerSet> {
        &self.peers
    }
}
