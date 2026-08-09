#![doc = include_str!("../README.md")]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the modules here are private, so pub(crate) and pub are equivalent -- pub(crate) is the one that says what is meant, and it is what rustc's unreachable_pub asks for"
)]

mod peer;
pub mod reliable;
mod wire;

use std::{
    collections::BTreeMap,
    io,
    net::{SocketAddr, ToSocketAddrs, UdpSocket},
    sync::Mutex,
    time::{Duration, Instant},
};

use corvid_net::{Channel, DATAGRAM_LIMIT, Delivery, Lost, PeerId, PeerSet, SendError, Transport};

use crate::peer::{Inner, Known, Ready};
use crate::wire::{
    HEADER, MAGIC, Parsed, VERSION, channel_code, channel_of, kind, parse, piece_body,
};

/// How often a peer that has not answered is greeted again.
const GREET: Duration = Duration::from_millis(250);

/// How long a peer may say nothing before it is considered gone.
///
/// Ten seconds. A lockstep session sends every tick, so a peer that is playing
/// says something thirty times a second and this is only ever reached by one
/// that has genuinely stopped.
///
/// Public because two pieces of this backend's own documentation are about what
/// happens when it runs out, and a number a reader is told about should be a
/// number they can name.
pub const PATIENCE: Duration = Duration::from_secs(10);

/// A socket, and the peers reachable through it.
///
/// ```no_run
/// use corvid_net::{PeerId, Transport};
/// use corvid_net_udp::UdpNet;
///
/// // One process binds a port and is told where the other is.
/// let here = UdpNet::bind(("0.0.0.0", 9000), PeerId(0))?;
/// here.connect(PeerId(1), "127.0.0.1:9001")?;
///
/// // From there it is the same trait every other backend implements.
/// here.send_datagram(PeerId(1), b"tick 41")?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug)]
pub struct UdpNet {
    /// The socket, non-blocking.
    socket: UdpSocket,
    /// The peers, and who is currently reachable among them.
    inner: Mutex<Inner>,
}

impl UdpNet {
    /// Binds a socket and answers a transport over it.
    ///
    /// `me` is the identity this process announces. It is chosen by whatever
    /// arranged the session rather than by the transport, exactly as
    /// [`PeerId`]'s own documentation says -- two processes started by one
    /// command line get it from the command line.
    ///
    /// # Errors
    ///
    /// Whatever the operating system says about binding that address, and about
    /// putting the socket into non-blocking mode.
    pub fn bind(address: impl ToSocketAddrs, me: PeerId) -> io::Result<Self> {
        let socket = UdpSocket::bind(address)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            inner: Mutex::new(Inner {
                me,
                peers: BTreeMap::new(),
                roster: PeerSet::new(),
            }),
        })
    }

    /// The address this socket ended up on, which a caller that asked for port
    /// zero needs.
    ///
    /// # Errors
    ///
    /// Whatever the operating system says.
    pub fn local(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// Says where a peer is, and starts greeting it.
    ///
    /// The peer is not in [`peers`](Transport::peers) until it answers, and it
    /// is not reachable until then either -- a `send_datagram` to it reports
    /// [`SendError::Unknown`]. That is the honest reading of "connected" for a
    /// protocol with no connection: this end knows an address, and has no
    /// evidence anything is there.
    ///
    /// # Errors
    ///
    /// Whatever resolving the address says, and
    /// [`AddrNotAvailable`](io::ErrorKind::AddrNotAvailable) for a name that
    /// resolves to nothing.
    pub fn connect(&self, peer: PeerId, address: impl ToSocketAddrs) -> io::Result<()> {
        let Some(address) = address.to_socket_addrs()?.next() else {
            return Err(io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "that name resolves to no address",
            ));
        };
        let now = Instant::now();
        {
            let mut inner = self.lock();
            inner.peers.insert(
                peer,
                Known {
                    address,
                    reachable: false,
                    greeted: now,
                    heard: now,
                    channels: BTreeMap::new(),
                },
            );
        }
        // Outside the lock, because sending takes the socket and not the table.
        self.post(address, kind::HELLO, &[]);
        Ok(())
    }

    /// Tells every peer this end is going, so the far end reports
    /// [`Lost::Closed`] rather than waiting out [`PATIENCE`].
    ///
    /// Called by [`Drop`], and public because a caller that wants the goodbye
    /// to have gone before it does something else needs to be able to ask.
    pub fn goodbye(&self) {
        let addresses: Vec<SocketAddr> = {
            let inner = self.lock();
            inner.peers.values().map(|known| known.address).collect()
        };
        for address in addresses {
            self.post(address, kind::BYE, &[]);
        }
    }

    /// The lock, with a poisoned one treated as an ordinary one.
    ///
    /// A panic while this lock was held is a bug in this file rather than a
    /// reason to make every later call panic too, and the workspace denies
    /// `unwrap` and `expect` alike -- so the recovery is to take the data back
    /// and carry on.
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Puts one packet on the wire, with the header on the front.
    ///
    /// A send that the operating system refuses is dropped. Every one of them
    /// is a packet lost, which is a thing this transport promises may happen to
    /// any packet at any time.
    fn post(&self, to: SocketAddr, kind: u8, body: &[u8]) {
        let me = self.lock().me;
        let mut packet = Vec::with_capacity(HEADER + body.len());
        packet.extend_from_slice(&MAGIC);
        packet.push(VERSION);
        packet.push(kind);
        packet.extend_from_slice(&me.0.to_le_bytes());
        packet.extend_from_slice(body);
        drop(self.socket.send_to(&packet, to));
    }

    /// Rebuilds the reachable set, after something moved a peer in or out.
    fn republish(&self) {
        let mut inner = self.lock();
        inner.roster = inner
            .peers
            .iter()
            .filter(|(_, known)| known.reachable)
            .map(|(peer, _)| *peer)
            .collect();
    }
}

impl Drop for UdpNet {
    /// Says goodbye. A peer told that its opponent has gone reports
    /// [`Lost::Closed`] on its next poll instead of playing on against a seat
    /// that will never speak again until [`PATIENCE`] runs out.
    fn drop(&mut self) {
        self.goodbye();
    }
}

impl Transport for UdpNet {
    fn send_datagram(&self, to: PeerId, bytes: &[u8]) -> Result<(), SendError> {
        if bytes.len() > DATAGRAM_LIMIT {
            return Err(SendError::TooLarge {
                bytes: bytes.len(),
                limit: DATAGRAM_LIMIT,
            });
        }
        let address = {
            let inner = self.lock();
            inner
                .peers
                .get(&to)
                .filter(|known| known.reachable)
                .map(|known| known.address)
        };
        let Some(address) = address else {
            return Err(SendError::Unknown(to));
        };
        self.post(address, kind::DATAGRAM, bytes);
        Ok(())
    }

    fn send_stream(&self, to: PeerId, channel: Channel, bytes: &[u8]) -> Result<(), SendError> {
        let code = channel_code(channel);
        let now = Instant::now();
        let queued = {
            let mut inner = self.lock();
            match inner.peers.get_mut(&to).filter(|known| known.reachable) {
                // A channel whose queue is full is a peer that has stopped
                // acknowledging, which from this end is indistinguishable from
                // one that has gone -- so the refusal is the same one.
                Some(known) => known
                    .channels
                    .entry(code)
                    .or_default()
                    .0
                    .send(bytes, now)
                    .map(|pieces| (known.address, pieces))
                    .map_err(|full| SendError::Backpressure {
                        waiting: full.waiting,
                        limit: full.limit,
                    }),
                None => Err(SendError::Unknown(to)),
            }
        };
        let (address, pieces) = queued?;
        for piece in &pieces {
            self.post(address, kind::PIECE, &piece_body(code, piece));
        }
        Ok(())
    }

    /// Reads everything that has arrived, answers it, and sends what is due.
    ///
    /// Three things happen here rather than one, and they are here rather than
    /// on a thread because a runtime calls this once a tick and a transport
    /// with a thread of its own would need a lock around every one of them
    /// anyway: packets are read and turned into deliveries, every channel that
    /// received something is acknowledged, and anything unacknowledged past
    /// [`reliable::RETRY`] goes again.
    fn poll(&self, sink: &mut dyn FnMut(PeerId, Delivery<'_>)) {
        let now = Instant::now();
        let mut ready: Vec<Ready> = Vec::new();
        // What has to be sent once the lock is released: acknowledgements,
        // welcomes and retransmissions.
        let mut outgoing: Vec<(SocketAddr, u8, Vec<u8>)> = Vec::new();
        let mut roster_moved = false;

        let mut buffer = [0_u8; 2048];
        loop {
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
                Parsed::Piece { code, piece } => {
                    let address = known.address;
                    let (_, receiver) = known.channels.entry(code).or_default();
                    let frames = receiver.accept(piece);
                    let mut acknowledgement = Vec::with_capacity(5);
                    acknowledgement.push(code);
                    acknowledgement.extend_from_slice(&receiver.expected().to_le_bytes());
                    outgoing.push((address, kind::ACK, acknowledgement));
                    if let Some(channel) = channel_of(code) {
                        ready.extend(
                            frames
                                .into_iter()
                                .map(|bytes| Ready::Frame(from, channel, bytes)),
                        );
                    }
                }
                Parsed::Ack { code, through } => {
                    let (sender, _) = known.channels.entry(code).or_default();
                    sender.acknowledged(through);
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

        roster_moved |= self.upkeep(now, &mut ready, &mut outgoing);

        for (address, kind, body) in outgoing {
            self.post(address, kind, &body);
        }
        if roster_moved {
            self.republish();
        }

        for delivery in &ready {
            match delivery {
                Ready::Joined(peer) => sink(*peer, Delivery::Joined),
                Ready::Lost(peer, because) => sink(*peer, Delivery::Lost { because: *because }),
                Ready::Datagram(peer, bytes) => sink(*peer, Delivery::Datagram(bytes)),
                Ready::Frame(peer, channel, bytes) => sink(
                    *peer,
                    Delivery::Stream {
                        channel: *channel,
                        bytes,
                    },
                ),
            }
        }
    }

    fn peers(&self) -> PeerSet {
        self.lock().roster.clone()
    }
}
