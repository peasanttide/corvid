//! Everything the network mutates, and the lock it is mutated behind.
//!
//! The public handles are in [`crate::net`]; this is what they call into. The
//! split is along the lock: nothing here takes it, every method assumes it is
//! already held, and `MockNet` is what holds it.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use corvid_net::{Channel, Link, PeerId, PeerSet, SendError};
use corvid_signal::{Emitter, Watch};

use crate::net::{INBOX, QUEUED};
use crate::queue::{Queue, Wait};
use crate::records::{Arrival, Inbox, LinkState, Pipe};
use crate::schedule::{Draws, Schedule, key};
use crate::tally::Tally;

/// Everything the network mutates, behind one lock.
#[derive(Debug)]
pub(crate) struct Inner {
    /// Virtual time since the network opened. Moved only by
    /// [`MockNet::advance`].
    pub(crate) now: Duration,
    /// What every draw is keyed by.
    pub(crate) seed: u64,
    /// Per direction, created on first use.
    pub(crate) links: BTreeMap<Link, LinkState>,
    /// Per direction and channel, created on first use.
    pub(crate) pipes: BTreeMap<(Link, Channel), Pipe>,
    /// Everything in flight, due-ordered.
    pub(crate) queue: Queue,
    /// Per peer, waiting to be polled.
    pub(crate) inboxes: Vec<Inbox>,
    /// Per peer, who it can currently reach.
    pub(crate) rosters: Vec<PeerSet>,
    /// The running count.
    pub(crate) tally: Tally,
}

/// What a network and its endpoints share.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) inner: Mutex<Inner>,
    /// Per peer, the published roster.
    pub(crate) emitters: Vec<Emitter<PeerSet>>,
    /// Per peer, the handle an endpoint hands back from
    /// [`Transport::peers`].
    pub(crate) watches: Vec<Watch<PeerSet>>,
}

impl Shared {
    /// The lock, with poisoning ignored.
    ///
    /// Nothing here runs a caller's code under this lock -- a `poll` sink runs
    /// after it is released, which is what lets a handler answer a packet from
    /// inside the loop -- so a poisoned lock means a panic somewhere that never
    /// touched this state.
    pub(crate) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Publishes one peer's roster, under the lock the change was made under.
    ///
    /// Reading the roster and then releasing the lock before setting the watch
    /// lets two changes interleave read-read-set-set, which leaves the watch
    /// advertising the older of the two for ever while `Inner` holds the
    /// newer. Holding it means the order rosters are published in is the order
    /// they were written in. Nothing takes these two locks the other way round.
    pub(crate) fn publish(&self, peer: PeerId) {
        let inner = self.lock();
        if let (Some(roster), Some(emitter)) = (
            inner.rosters.get(usize::from(peer.to_u16())),
            self.emitters.get(usize::from(peer.to_u16())),
        ) {
            emitter.set(roster.clone());
        }
        drop(inner);
    }
}

impl Inner {
    /// Whether `from` can currently reach `to`.
    pub(crate) fn connected(&self, link: Link) -> bool {
        self.rosters
            .get(usize::from(link.from.to_u16()))
            .is_some_and(|roster| roster.contains(link.to))
    }

    /// The link a send is allowed to use.
    pub(crate) fn route(&self, from: PeerId, to: PeerId) -> Result<Link, SendError> {
        let link = Link::new(from, to);
        if self.connected(link) {
            Ok(link)
        } else {
            Err(SendError::Unknown(to))
        }
    }

    /// The next draw stream for one link, bumping its counter.
    pub(crate) fn draw(&mut self, link: Link) -> (Schedule, Draws) {
        let seed = self.seed;
        let state = self.links.entry(link).or_default();
        let rng = Draws::new(seed, key(link), state.draws);
        state.draws += 1;
        (state.schedule, rng)
    }

    /// Puts one arrival in a peer's inbox.
    pub(crate) fn post(&mut self, to: PeerId, from: PeerId, arrival: Arrival) {
        let Some(inbox) = self.inboxes.get_mut(usize::from(to.to_u16())) else {
            return;
        };
        inbox.push(from, arrival);

        // A peer nobody polls is a peer whose inbox grows for ever, and a test
        // double is the last place a run should end in the allocator. What a
        // real socket does when nothing reads its receive buffer is drop the
        // oldest, so that is what this does.
        //
        // Traffic only, on both sides of the comparison. [`Delivery::Joined`]
        // says it precedes everything else from that peer, so dropping the
        // oldest arrival would drop exactly that -- and counting joins toward
        // the allowance would be worse still, because a session with more
        // peers than the bound starts every inbox over it and nothing sent
        // could ever be read.
        while inbox.traffic > INBOX {
            let Some(oldest) = inbox
                .waiting
                .iter()
                .position(|(_, arrival)| arrival.is_droppable())
            else {
                // Nothing here is a datagram, so there is nothing this is
                // willing to drop.
                break;
            };
            drop(inbox.remove(oldest));
            // Off `delivered`, which is read as what a sink can still see, and
            // on to `dropped`, which is where a datagram the receiver will
            // never get belongs. Otherwise an evicted one is in neither and
            // the tally does not add up.
            self.tally.delivered = self.tally.delivered.saturating_sub(1);
            self.tally.dropped += 1;
        }
    }

    /// Draws one datagram's fate and queues it.
    pub(crate) fn offer(&mut self, link: Link, bytes: Vec<u8>) {
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
        // Only when this one would otherwise land at or after the neighbour.
        // Where jitter has already put it in front, there is no crossing left
        // to make, and assigning the neighbour's instant would *delay* it --
        // counting a reorder for having slowed traffic down.
        if across && neighbour > now && due >= neighbour {
            due = neighbour.saturating_sub(Duration::from_nanos(1)).max(now);
            self.tally.reordered += 1;
        }

        if let Some(state) = self.links.get_mut(&link) {
            // The *later* of the two, so a reordered datagram displaces one
            // neighbour and the next draw is measured against where the burst
            // actually reaches. Keeping the reordered instant here instead
            // walked the mark backwards on every hit, so a link at
            // `reorder: ONE` delivered a twenty-datagram burst exactly
            // reversed rather than shuffled -- which is not what one crossing
            // means, and is what `Tally::reordered` counts one of.
            state.last_due = state.last_due.max(due);
        }
        self.tally.in_flight += 1;
        self.queue.push(due, Wait::Datagram { link, bytes });
    }

    /// Arms one delivery attempt at the head of a pipe, leaving at `base`.
    pub(crate) fn attempt(&mut self, link: Link, ch: Channel, base: Duration) {
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
    pub(crate) fn write(&mut self, link: Link, ch: Channel, bytes: &[u8]) -> Result<(), SendError> {
        let pipe = self.pipes.entry((link, ch)).or_default();
        if pipe.frames.len() >= QUEUED {
            return Err(SendError::Backpressure {
                waiting: pipe.frames.len(),
                limit: QUEUED,
            });
        }

        // Copied after the check and not before it. A refusal that first
        // allocates a copy of the frame it is refusing is a refusal that can
        // fail to arrive, and the frames this channel carries are the large
        // ones.
        pipe.frames.push_back(bytes.to_vec());
        self.tally.sent += 1;
        self.tally.in_flight += 1;

        let pipe = self.pipes.entry((link, ch)).or_default();
        let idle = !pipe.armed;
        pipe.armed = true;

        if idle {
            let now = self.now;
            self.attempt(link, ch, now);
        }
        Ok(())
    }

    /// Delivers everything due at the clock's current position.
    pub(crate) fn deliver(&mut self) {
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
    pub(crate) fn arrive(&mut self, link: Link, ch: Channel, lost: bool, due: Duration) {
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

        // A reliable frame is never dropped, so a receiver that has stopped
        // polling has to be told to wait rather than have its inbox overrun.
        // The frame stays at the head of the pipe and the attempt comes round
        // again, which is a closed receive window and is what a real reliable
        // transport does with one.
        if self
            .inboxes
            .get(usize::from(link.to.to_u16()))
            .is_some_and(|inbox| inbox.traffic >= INBOX)
        {
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
    pub(crate) fn discard(&mut self, link: Link) -> u32 {
        let mut lost = 0;
        self.queue.retain(|wait| {
            let mine = wait.link() == link;
            if mine && matches!(wait, Wait::Datagram { .. }) {
                lost += 1;
            }
            !mine
        });
        // Every pipe on this link, in one pass, rather than a lookup per
        // `Channel::ALL` entry. What a cut has to forget is defined by the
        // link and not by the channel list, so saying that directly leaves
        // nothing to keep level with anything else.
        self.pipes.retain(|(pipe_link, _), pipe| {
            let mine = *pipe_link == link;
            if mine {
                lost += u32::try_from(pipe.frames.len()).unwrap_or(u32::MAX);
            }
            !mine
        });
        lost
    }
}
