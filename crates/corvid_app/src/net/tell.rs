//! Saying something to one peer or to all of them.
//!
//! The seam is that nothing here decides anything: these are the sends the
//! three files beside it reach for, so encoding a control message and counting
//! what went out is written down once.

use corvid_behavior::{PlayerId, State};
use corvid_net::{Channel, PeerId, SendError};

use crate::net::{Control, Link, TickTraffic};

impl<S: State> Link<S> {
    /// Tells one peer what this machine has said about every seat.
    pub(super) fn tell(&self, peer: PeerId, me: PlayerId) {
        for (seat, at) in &self.mine {
            self.say(
                peer,
                Control::Departed {
                    seat: seat.0,
                    from: me.0,
                    at: *at,
                },
            );
        }
    }

    /// One control message to everybody the transport can reach.
    pub(super) fn say_all(&self, control: Control) {
        for peer in self.transport.peers().iter() {
            self.say(peer, control);
        }
    }

    /// One control message, to one peer, reliably.
    ///
    /// Nothing here can stop the run. A control frame that will not go is a
    /// peer that has gone, and what this runtime does about a peer that has
    /// gone is the message it was trying to send.
    pub(super) fn say(&self, to: PeerId, control: Control) {
        let Ok(bytes) = corvid_wire::encode(&control) else {
            tracing::error!(
                name: "corvid_app.unencodable_control",
                "a control message could not be encoded, so this peer says nothing",
            );
            return;
        };
        if let Err(why) = self.transport.send_stream(to, Channel::Control, &bytes) {
            tracing::debug!(
                name: "corvid_app.unsent_control",
                peer = %to,
                why = %why,
                "this control message did not go",
            );
        }
    }

    /// Sends this peer's newest window of actions and its digest to everyone.
    ///
    /// Nothing here can stop the run. A send that fails is a peer that has gone
    /// or a path that will not carry the frame, and a lockstep session's answer
    /// to both is the same one it has for a lost packet: predict, and correct
    /// when something arrives.
    pub(super) fn broadcast(&mut self, traffic: &mut TickTraffic) {
        let datagram = self.peer.outgoing();
        self.outbound.clear();
        match corvid_wire::encode(&datagram) {
            Ok(bytes) => self.outbound = bytes,
            Err(why) => {
                tracing::error!(
                    name: "corvid_app.unencodable",
                    why = %why,
                    "this peer's own datagram could not be encoded, so this tick says nothing",
                );
                return;
            }
        }

        let peers = self.transport.peers();
        traffic.peers = u16::try_from(peers.len()).unwrap_or(u16::MAX);
        for peer in peers.iter() {
            match self.transport.send_datagram(peer, &self.outbound) {
                Ok(()) => traffic.sent = traffic.sent.saturating_add(1),
                Err(SendError::TooLarge { bytes, limit }) => tracing::error!(
                    name: "corvid_app.oversized",
                    peer = %peer,
                    bytes,
                    limit,
                    "this session's action window does not fit in one datagram, so this \
                     peer is hearing nothing from this machine",
                ),
                Err(why) => tracing::debug!(
                    name: "corvid_app.unsent",
                    peer = %peer,
                    why = %why,
                    "this tick's datagram did not go; the other end predicts through it",
                ),
            }
        }
    }
}
