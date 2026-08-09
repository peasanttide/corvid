//! Reading the socket: what one poll takes off it, and what that becomes.
//!
//! Split from [`UdpNet`] itself because the two halves of a poll have
//! nothing to say to each other beyond three arguments -- this one only
//! reads, and the other only drains what this produced.

use std::{collections::BTreeMap, io, net::SocketAddr, time::Instant};

use corvid_net::Lost;

use crate::peer::{Known, Ready};
use crate::wire::{Parsed, channel_of, kind, parse};
use crate::{PER_POLL, UdpNet};

impl UdpNet {
    /// Reads what the socket has, up to [`PER_POLL`] packets, and turns each
    /// into deliveries and replies. Answers whether the roster moved.
    ///
    /// Split out of [`poll`](Transport::poll) because the two halves have
    /// nothing to say to each other beyond these three arguments: this one
    /// only reads the socket, and the other only drains what it produced.
    pub(crate) fn receive(
        &self,
        now: Instant,
        ready: &mut Vec<Ready>,
        outgoing: &mut Vec<(SocketAddr, u8, Vec<u8>)>,
    ) -> bool {
        let mut roster_moved = false;
        let mut buffer = [0_u8; 2048];
        for _ in 0..PER_POLL {
            let (read, from_address) = match self.socket.recv_from(&mut buffer) {
                Ok(read) => read,
                // Nothing more to read, which is what a non-blocking socket
                // says when it is empty.
                Err(why) if why.kind() == io::ErrorKind::WouldBlock => break,
                // On Windows a datagram larger than the buffer, and on any
                // platform an ICMP answer to an earlier send, arrive as errors
                // on `recv_from`. Neither says anything about the packets
                // behind them, so the loop carries on rather than stopping and
                // leaving them queued.
                Err(_) => continue,
            };
            let Some(packet) = buffer.get(..read) else {
                continue;
            };
            let Some((from, what)) = parse(packet) else {
                continue;
            };
            // The third way in, and the one this process does not control. A
            // packet claiming to come from nobody would otherwise reach the
            // peer table and be published in a roster, which is the sentinel
            // escaping through the door the other two now shut.
            if from.is_none() {
                continue;
            }

            let mut inner = self.lock();
            if from == inner.me {
                // This process's own packet, looped back. Nothing sends to
                // itself deliberately, and a session where two peers claimed
                // one identity would be a session where each rolled back
                // against its own actions.
                continue;
            }
            let known = inner.peers.entry(from).or_insert_with(|| Known {
                address: from_address,
                reachable: false,
                greeted: now,
                heard: now,
                channels: BTreeMap::new(),
            });
            // The address a peer is actually speaking from wins over the one
            // this end was told, which is what makes a peer behind a router
            // reachable once it has spoken first.
            known.address = from_address;
            known.heard = now;

            match what {
                Parsed::Greeting { welcome } => {
                    if !known.reachable {
                        known.reachable = true;
                        roster_moved = true;
                        ready.push(Ready::Joined(from));
                    }
                    if !welcome {
                        // Answered every time rather than once: a welcome may
                        // be the packet that is lost, and the far end greets
                        // again until something arrives.
                        outgoing.push((known.address, kind::WELCOME, Vec::new()));
                    }
                }
                Parsed::Datagram(bytes) => {
                    if known.reachable {
                        ready.push(Ready::Datagram(from, bytes.to_vec()));
                    }
                }
                // The channel is resolved before any state is made for it. A
                // code this build cannot name buys a stranger a receiver and
                // an acknowledgement otherwise, for traffic that could never
                // be delivered -- and `reachable` is what separates a peer
                // that greeted from one that merely knows the port.
                Parsed::Piece { code, piece } => {
                    if let (true, Some(channel)) = (known.reachable, channel_of(code)) {
                        let address = known.address;
                        let (_, receiver) = known.channels.entry(code).or_default();
                        let frames = receiver.accept(piece);
                        let mut acknowledgement = Vec::with_capacity(5);
                        acknowledgement.push(code);
                        acknowledgement.extend_from_slice(&receiver.expected().to_le_bytes());
                        outgoing.push((address, kind::ACK, acknowledgement));
                        ready.extend(
                            frames
                                .into_iter()
                                .map(|bytes| Ready::Frame(from, channel, bytes)),
                        );
                    }
                }
                // `get_mut` rather than `entry`: an acknowledgement is about a
                // sender that exists, and one arriving after a goodbye would
                // otherwise both allocate a channel and discard what is queued
                // on it.
                Parsed::Ack { code, through } => {
                    if known.reachable
                        && let Some((sender, _)) = known.channels.get_mut(&code)
                    {
                        sender.acknowledged(through);
                    }
                }
                Parsed::Bye => {
                    if known.reachable {
                        known.reachable = false;
                        roster_moved = true;
                        ready.push(Ready::Lost(from, Lost::Closed));
                    }
                }
            }
            drop(inner);
        }
        roster_moved
    }
}
