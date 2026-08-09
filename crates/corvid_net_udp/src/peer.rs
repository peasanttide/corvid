//! What the backend remembers about the far ends, and the pass that keeps
//! it current.
//!
//! A peer is greeted until it answers and dropped once it has said nothing
//! for [`PATIENCE`]. Both edges are decided here and reported by
//! [`UdpNet::poll`](crate::UdpNet::poll), so the socket half never has to ask
//! whether a peer is still there.

use std::{collections::BTreeMap, net::SocketAddr, time::Instant};

use corvid_net::{Channel, Lost, PeerId, PeerSet};

use crate::reliable::{Receiver, Sender};
use crate::wire::{kind, piece_body};
use crate::{GREET, PATIENCE, UdpNet};

#[derive(Debug)]
pub(crate) struct Known {
    /// Where it is.
    pub(crate) address: SocketAddr,
    /// Whether it has answered, which is what puts it in the published
    /// [`PeerSet`].
    pub(crate) reachable: bool,
    /// When it was last greeted, for a peer that has not answered yet.
    pub(crate) greeted: Instant,
    /// When anything was last heard from it.
    pub(crate) heard: Instant,
    /// The reliable channels, by code.
    pub(crate) channels: BTreeMap<u8, (Sender, Receiver)>,
}

/// Everything behind the lock.
#[derive(Debug)]
pub(crate) struct Inner {
    /// Who this process is.
    pub(crate) me: PeerId,
    /// Everyone it has been told about or heard from.
    pub(crate) peers: BTreeMap<PeerId, Known>,
    /// The reachable subset of the above, kept rather than derived.
    ///
    /// `Transport::peers` answers a snapshot and a runtime asks once a tick,
    /// where the greetings and timeouts that move a peer in or out happen far
    /// less often than that. Recomputing per call would walk the table on
    /// every tick to produce the same set it produced last tick.
    pub(crate) roster: PeerSet,
}

/// What one poll found, ready to hand to the sink once the lock is released.
pub(crate) enum Ready {
    /// A peer became reachable.
    Joined(PeerId),
    /// A peer went away.
    Lost(PeerId, Lost),
    /// Unreliable bytes.
    Datagram(PeerId, Vec<u8>),
    /// A whole reassembled frame on a channel.
    Frame(PeerId, Channel, Vec<u8>),
}

impl UdpNet {
    /// Greets the peers that have not answered, gives up on the ones that have
    /// gone quiet, and sends whatever a far end has not acknowledged.
    ///
    /// Answers whether the roster moved. Its own method rather than the tail of
    /// [`poll`](Transport::poll) because it is the half that happens whether or
    /// not anything arrived: a link that is down produces no packets to read
    /// and is exactly when the greeting and the timeout matter.
    pub(crate) fn upkeep(
        &self,
        now: Instant,
        ready: &mut Vec<Ready>,
        outgoing: &mut Vec<(SocketAddr, u8, Vec<u8>)>,
    ) -> bool {
        let mut moved = false;
        // Taken and dropped explicitly, because a guard held to the end of a
        // function is a lock held for longer than the work that needs it -- and
        // this one is taken again by `post` the moment the caller sends what
        // this filled in.
        let mut inner = self.lock();
        for (peer, known) in &mut inner.peers {
            if !known.reachable {
                if now.duration_since(known.greeted) >= GREET {
                    known.greeted = now;
                    outgoing.push((known.address, kind::HELLO, Vec::new()));
                }
                continue;
            }
            if now.duration_since(known.heard) >= PATIENCE {
                known.reachable = false;
                moved = true;
                ready.push(Ready::Lost(*peer, Lost::TimedOut));
                continue;
            }
            for (code, (sender, _)) in &mut known.channels {
                for piece in sender.due(now) {
                    outgoing.push((known.address, kind::PIECE, piece_body(*code, &piece)));
                }
            }
        }
        drop(inner);
        moved
    }
}
