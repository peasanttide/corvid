//! Peers linked in-process, with scriptable latency, jitter, loss and reorder.

mod queue;
mod schedule;

use std::{
    collections::{BTreeMap, VecDeque},
    mem,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use corvid_signal::{Emitter, Watch, channel};

use self::queue::{Link, Queue, Wait};
use self::schedule::Rng;
pub use self::schedule::Schedule;
use crate::{Channel, DATAGRAM_LIMIT, Delivery, Lost, PeerId, PeerSet, SendError, Transport};

/// What has happened, for a lab's graph and a test's assertion.
///
/// `sent` counts datagrams and stream frames the network accepted; `delivered`
/// counts the ones that reached an inbox. A join or a loss is a connection
/// event rather than traffic and is in neither.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Tally {
    /// Datagrams and stream frames handed to the network.
    pub sent: u64,
    /// Datagrams and stream frames placed in an inbox.
    pub delivered: u64,
    /// Datagrams lost outright, plus stream attempts that were lost and
    /// retried.
    pub dropped: u64,
    /// Datagrams whose delivery instant was moved across an in-flight
    /// neighbour's.
    pub reordered: u64,
    /// How much is waiting to be delivered right now.
    pub in_flight: u32,
}

/// What is sitting in one peer's inbox, owned until it is polled.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Arrival {
    Datagram(Vec<u8>),
    Stream { channel: Channel, bytes: Vec<u8> },
    Joined,
    Lost(Lost),
}

impl Arrival {
    /// The borrowed view a [`Transport::poll`] sink is handed.
    fn delivery(&self) -> Delivery<'_> {
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

/// One reliable channel in one direction.
///
/// Only the frame at the head is ever in flight, so a retransmit holds
/// everything behind it — which is what makes a state transfer over a bad link
/// take a visible amount of time rather than an averaged one.
#[derive(Debug, Default)]
struct Pipe {
    /// Written but not yet delivered, oldest first.
    frames: VecDeque<Vec<u8>>,
    /// Whether an attempt at the head is already queued.
    armed: bool,
}

/// One direction of one link, as the network holds it.
#[derive(Clone, Copy, Debug)]
struct LinkState {
    /// The curve it follows.
    schedule: Schedule,
    /// How many scheduling decisions this link's draw stream has served.
    draws: u64,
    /// The due of the last datagram scheduled here — the neighbour a reordered
    /// one is moved across.
    last_due: Duration,
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

/// Everything the network mutates, behind one lock.
#[derive(Debug)]
struct Inner {
    /// Virtual time since the network opened. Moved only by
    /// [`MockNet::advance`].
    now: Duration,
    /// What every draw is keyed by.
    seed: u64,
    /// Per direction, created on first use.
    links: BTreeMap<Link, LinkState>,
    /// Per direction and channel, created on first use.
    pipes: BTreeMap<(Link, Channel), Pipe>,
    /// Everything in flight, due-ordered.
    queue: Queue,
    /// Per peer, waiting to be polled.
    inboxes: Vec<VecDeque<(PeerId, Arrival)>>,
    /// Per peer, who it can currently reach.
    rosters: Vec<PeerSet>,
    /// The running count.
    tally: Tally,
}

/// What a network and its endpoints share.
#[derive(Debug)]
struct Shared {
    inner: Mutex<Inner>,
    /// Per peer, the published roster.
    emitters: Vec<Emitter<PeerSet>>,
    /// Per peer, the handle an endpoint hands back from
    /// [`Transport::peers`].
    watches: Vec<Watch<PeerSet>>,
}

impl Shared {
    /// The lock, with poisoning ignored.
    ///
    /// Nothing here runs a caller's code under this lock — a `poll` sink runs
    /// after it is released, which is what lets a handler answer a packet from
    /// inside the loop — so a poisoned lock means a panic somewhere that never
    /// touched this state.
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Publishes one peer's roster.
    fn publish(&self, peer: PeerId) {
        let roster = {
            let inner = self.lock();
            inner.rosters.get(usize::from(peer.to_u16())).cloned()
        };
        if let (Some(roster), Some(emitter)) =
            (roster, self.emitters.get(usize::from(peer.to_u16())))
        {
            emitter.set(roster);
        }
    }
}

impl Inner {
    /// Whether `from` can currently reach `to`.
    fn connected(&self, link: Link) -> bool {
        self.rosters
            .get(usize::from(link.from.to_u16()))
            .is_some_and(|roster| roster.contains(link.to))
    }

    /// The link a send is allowed to use.
    fn route(&self, from: PeerId, to: PeerId) -> Result<Link, SendError> {
        let link = Link::new(from, to);
        if self.connected(link) {
            Ok(link)
        } else {
            Err(SendError::Unknown(to))
        }
    }

    /// The next draw stream for one link, bumping its counter.
    fn draw(&mut self, link: Link) -> (Schedule, Rng) {
        let seed = self.seed;
        let state = self.links.entry(link).or_default();
        let rng = Rng::new(seed, link.key(), state.draws);
        state.draws += 1;
        (state.schedule, rng)
    }

    /// Puts one arrival in a peer's inbox.
    fn post(&mut self, to: PeerId, from: PeerId, arrival: Arrival) {
        if let Some(inbox) = self.inboxes.get_mut(usize::from(to.to_u16())) {
            inbox.push_back((from, arrival));
            // A peer nobody polls is a peer whose inbox grows for ever, and a
            // test double is the last place a run should end in the allocator.
            // What a real socket does when nothing reads its receive buffer is
            // drop the oldest, so that is what this does — and it is a *fact
            // about a peer that stopped polling* rather than about the link, so
            // it is not counted as loss.
            while inbox.len() > INBOX {
                drop(inbox.pop_front());
            }
        }
    }

    /// Draws one datagram's fate and queues it.
    fn offer(&mut self, link: Link, bytes: Vec<u8>) {
        self.tally.sent += 1;

        let now = self.now;
        let (schedule, mut rng) = self.draw(link);

        if rng.hits(schedule.loss) {
            self.tally.dropped += 1;
            return;
        }

        let trip = schedule.latency.saturating_add(rng.spread(schedule.jitter));
        let mut due = now.saturating_add(trip);

        // Drawn whether or not it can be used, so the stream stays in step: a
        // schedule's draws must not depend on what else is in flight.
        let across = rng.hits(schedule.reorder);
        let neighbour = self
            .links
            .get(&link)
            .map_or(Duration::ZERO, |it| it.last_due);
        if across && neighbour > now {
            due = neighbour.saturating_sub(Duration::from_nanos(1)).max(now);
            self.tally.reordered += 1;
        }

        if let Some(state) = self.links.get_mut(&link) {
            state.last_due = due;
        }
        self.tally.in_flight += 1;
        self.queue.push(due, Wait::Datagram { link, bytes });
    }

    /// Arms one delivery attempt at the head of a pipe, leaving at `base`.
    fn attempt(&mut self, link: Link, ch: Channel, base: Duration) {
        let (schedule, mut rng) = self.draw(link);
        let lost = rng.hits(schedule.loss);
        let trip = schedule.latency.saturating_add(rng.spread(schedule.jitter));
        self.queue.push(
            base.saturating_add(trip),
            Wait::Attempt {
                link,
                channel: ch,
                lost,
            },
        );
    }

    /// Writes one frame into a reliable pipe, arming it if it was idle.
    fn write(&mut self, link: Link, ch: Channel, bytes: Vec<u8>) {
        self.tally.sent += 1;
        self.tally.in_flight += 1;

        let pipe = self.pipes.entry((link, ch)).or_default();
        pipe.frames.push_back(bytes);
        let idle = !pipe.armed;
        pipe.armed = true;

        if idle {
            let now = self.now;
            self.attempt(link, ch, now);
        }
    }

    /// Delivers everything due at the clock's current position.
    fn deliver(&mut self) {
        while let Some(pending) = self.queue.pop_due(self.now) {
            match pending.wait {
                Wait::Datagram { link, bytes } => {
                    self.tally.in_flight = self.tally.in_flight.saturating_sub(1);
                    if self.connected(link) {
                        self.tally.delivered += 1;
                        self.post(link.to, link.from, Arrival::Datagram(bytes));
                    }
                }
                Wait::Attempt {
                    link,
                    channel: ch,
                    lost,
                } => self.arrive(link, ch, lost, pending.due),
            }
        }
    }

    /// One attempt at the head of a pipe, come due.
    fn arrive(&mut self, link: Link, ch: Channel, lost: bool, due: Duration) {
        if !self.connected(link) {
            return;
        }

        if lost {
            self.tally.dropped += 1;
            let retransmit = self
                .links
                .get(&link)
                .map_or(Schedule::PERFECT, |it| it.schedule)
                .retransmit();
            self.attempt(link, ch, due.saturating_add(retransmit));
            return;
        }

        let Some(pipe) = self.pipes.get_mut(&(link, ch)) else {
            return;
        };
        let frame = pipe.frames.pop_front();
        let more = !pipe.frames.is_empty();
        pipe.armed = more;

        if let Some(bytes) = frame {
            self.tally.delivered += 1;
            self.tally.in_flight = self.tally.in_flight.saturating_sub(1);
            self.post(link.to, link.from, Arrival::Stream { channel: ch, bytes });
        }

        // From the instant this frame landed rather than from the clock's
        // current position, so a pipe drains at the link's rate however coarse
        // the steps driving it are.
        if more {
            self.attempt(link, ch, due);
        }
    }

    /// Forgets everything in flight one way, and answers how many datagrams
    /// and frames went with it.
    fn discard(&mut self, link: Link) -> u32 {
        let mut lost = 0;
        self.queue.retain(|wait| {
            let mine = wait.link() == link;
            if mine && matches!(wait, Wait::Datagram { .. }) {
                lost += 1;
            }
            !mine
        });
        for ch in Channel::ALL {
            if let Some(pipe) = self.pipes.remove(&(link, ch)) {
                lost += u32::try_from(pipe.frames.len()).unwrap_or(u32::MAX);
            }
        }
        lost
    }
}

/// Peers linked in-process through a pipe, with scriptable latency, jitter,
/// loss and reorder.
///
/// Public API rather than a test helper: a netcode lab and a netcode test are
/// this with different assertions, and a downstream game builds its own netcode
/// tests out of it.
///
/// Nothing is delivered by a thread. The clock moves only in
/// [`advance`](Self::advance), so a test drives it a step at a time and a lab
/// drives it from its frame loop, and the two are the same code.
///
/// ```
/// use core::time::Duration;
///
/// use corvid_net::{Delivery, MockNet, PeerId, Schedule, Transport as _};
///
/// let net = MockNet::new(2, 0x5eed);
/// net.all(Schedule::DOMESTIC);
///
/// let alice = net.endpoint(PeerId(0));
/// let bob = net.endpoint(PeerId(1));
///
/// alice.send_datagram(PeerId(1), b"tick 41")?;
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
/// assert_eq!(heard, [(PeerId(0), b"tick 41".to_vec())]);
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
        let count = usize::from(peers);
        let mut rosters: Vec<PeerSet> = Vec::with_capacity(count);
        let mut inboxes: Vec<VecDeque<(PeerId, Arrival)>> = Vec::with_capacity(count);

        for me in 0..peers {
            rosters.push((0..peers).filter(|&it| it != me).map(PeerId).collect());
            inboxes.push(
                (0..peers)
                    .filter(|&it| it != me)
                    .map(|it| (PeerId(it), Arrival::Joined))
                    .collect(),
            );
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
    #[must_use]
    pub fn peers(&self) -> u16 {
        u16::try_from(self.shared.watches.len()).unwrap_or(u16::MAX)
    }

    /// One peer's handle. Hand it to whatever is playing that seat; it
    /// implements [`Transport`].
    ///
    /// A peer this network does not have — [`PeerId::NONE`] among them — gets
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
        for from in 0..peers {
            for to in 0..peers {
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
    /// changes no tally — including on a link with no latency at all, where
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

/// The most arrivals one peer's inbox holds before the oldest are dropped.
///
/// A thousand is far more than a peer polling once a tick ever accumulates —
/// at thirty ticks a second and one datagram per peer per tick, it is half a
/// minute of traffic from a peer that has stopped reading.
pub const INBOX: usize = 1_000;

/// One peer's view of a [`MockNet`].
///
/// Cheap to clone, and every clone is the same seat.
#[derive(Clone, Debug)]
pub struct Endpoint {
    net: Arc<Shared>,
    me: PeerId,
    peers: Watch<PeerSet>,
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
        inner.write(link, channel, bytes.to_vec());
        drop(inner);
        Ok(())
    }

    fn poll(&self, sink: &mut dyn FnMut(PeerId, Delivery<'_>)) {
        // Taken whole and under the lock, then handed out with the lock
        // released — which is what lets a sink send, cut a link or advance the
        // clock from inside the loop.
        let arrivals = {
            let mut inner = self.net.lock();
            inner
                .inboxes
                .get_mut(usize::from(self.me.to_u16()))
                .map(mem::take)
                .unwrap_or_default()
        };

        for (from, arrival) in &arrivals {
            sink(*from, arrival.delivery());
        }
    }

    fn peers(&self) -> &Watch<PeerSet> {
        &self.peers
    }
}
