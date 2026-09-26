//! The lobby's socket, lent to the session it started.

use std::sync::Arc;
use std::vec::Vec;

use corvid_net::{Channel, Delivery, PeerId, Transport};
use corvid_net_udp::UdpNet;

use super::say::Say;

/// The lobby's socket, as the transport a session's link plays over.
#[derive(Debug)]
pub(crate) struct Shared(pub(crate) Arc<UdpNet>);

impl Transport for Shared {
    fn send_datagram(&self, to: PeerId, bytes: &[u8]) -> Result<(), corvid_net::SendError> {
        self.0.send_datagram(to, bytes)
    }

    fn send_stream(
        &self,
        to: PeerId,
        channel: Channel,
        bytes: &[u8],
    ) -> Result<(), corvid_net::SendError> {
        self.0.send_stream(to, channel, bytes)
    }

    fn poll(&self, sink: &mut dyn FnMut(PeerId, Delivery<'_>)) {
        self.0.poll(sink);
    }

    fn peers(&self) -> corvid_net::PeerSet {
        self.0.peers()
    }

    fn datagram_limit(&self) -> usize {
        self.0.datagram_limit()
    }
}

/// Sends every guest the terms of the opening a host is starting from.
pub(crate) fn announce(transport: &dyn Transport, guests: &[PeerId], terms: Vec<u8>) {
    let Ok(bytes) = corvid_wire::encode(&Say::Start { terms }) else {
        tracing::error!(name: "corvid_app.lobby_unencoded", "the terms could not be encoded, so no guest can start");
        return;
    };
    for guest in guests {
        if let Err(why) = transport.send_stream(*guest, Channel::Opening, &bytes) {
            tracing::error!(name: "corvid_app.lobby_unstarted", peer = %guest, %why, "a guest was not sent the terms");
        }
    }
}
