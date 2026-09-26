//! Gathering machines into a session, and back again.
//!
//! One machine hosts: it opens a [`Lobby`] on a port, and shouts about it on
//! the local network. Others join it, by the address a [`Browser`] found or by
//! one typed in. The host seats everyone as they arrive, each guest says when
//! it is ready, and the host starts the session: every guest is sent the
//! terms of the opening the host is playing from, and every machine's loop
//! plays that opening over the lobby's socket in lockstep from its next tick.
//! When the session ends by the game's say-so, everyone comes back here,
//! still connected, to start another; a machine that leaves says so and goes.
//!
//! The runtime owns the lobby and drives it from the requests a game's
//! controller makes (see `runtime/net.rs`). The socket is shared: the lobby
//! reads it while gathering, and the session's link while linked.

use std::collections::BTreeMap;
use std::io;
use std::net::SocketAddr;
use std::string::{String, ToString};
use std::sync::Arc;
use std::vec::Vec;

use corvid_behavior::PlayerId;
use corvid_net::{Channel, Delivery, PeerId, Transport};
use corvid_net_udp::UdpNet;

mod beacon;
mod guest;
mod host;
mod say;

pub(crate) use beacon::Browser;

use beacon::Shouter;
use say::Say;

/// The peer a host always is.
const HOST: PeerId = PeerId(1);

/// One machine in a lobby.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Member {
    /// Which machine.
    pub(crate) peer: PeerId,
    /// What it calls itself.
    pub(crate) name: String,
    /// The seat it will play.
    pub(crate) seat: PlayerId,
    /// Whether it is ready to start. The host always is.
    pub(crate) ready: bool,
    address: Option<String>,
}

/// Where a lobby has got to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    /// Waiting for people, or for the host.
    Gathering,
    /// The host turned this machine away, and said why.
    Refused(String),
    /// The session has started, and the loop is playing it.
    Linked,
    /// The host went away.
    Closed,
}

/// A session a lobby has started, for the loop to play.
#[derive(Debug)]
pub(crate) struct Started {
    /// The seat this machine plays.
    pub(crate) seat: PlayerId,
    /// Which machine plays which seat.
    pub(crate) seats: BTreeMap<PeerId, PlayerId>,
    /// How many seats the session has.
    pub(crate) width: u16,
    /// The terms the host sent, encoded; [`None`] on the host itself, which
    /// writes them from the session it is playing and sends them to
    /// [`guests`](Self::guests).
    pub(crate) terms: Option<Vec<u8>>,
    /// Who the host sends the terms to. Empty on a guest.
    pub(crate) guests: Vec<PeerId>,
}

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

/// A lobby, hosted here or joined.
#[derive(Debug)]
pub(crate) struct Lobby {
    /// The socket, shared with the session's link while linked.
    net: Arc<UdpNet>,
    /// A session this lobby started that the loop has not taken yet.
    began: Option<Started>,
    me: PeerId,
    game: String,
    name: String,
    seats: u16,
    members: Vec<Member>,
    stage: Stage,
    /// A host's beacon.
    shouter: Option<Shouter>,
    /// Whether a guest has said hello yet.
    greeted: bool,
}

/// A peer number for a guest: anything but nobody and the host, and unlikely
/// to be another guest's.
fn guest_id() -> PeerId {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let mixed = nanos ^ std::process::id().rotate_left(16);
    let low = u16::try_from(mixed % 65_000).unwrap_or(0);
    PeerId(low.saturating_add(2))
}

impl Lobby {
    /// Hosts a lobby for `game` on `port` -- 0 for any free one -- with
    /// `seats` seats, the first of them this machine's.
    ///
    /// # Errors
    ///
    /// Whatever binding the port says.
    pub(crate) fn host(port: u16, name: &str, game: &str, seats: u16) -> io::Result<Self> {
        let net = UdpNet::bind(("0.0.0.0", port), HOST)?;
        let host = Member {
            peer: HOST,
            name: name.to_string(),
            seat: PlayerId(0),
            ready: true,
            address: None,
        };
        Ok(Self {
            net: Arc::new(net),
            began: None,
            me: HOST,
            game: game.to_string(),
            name: name.to_string(),
            seats: seats.max(1),
            members: std::vec![host],
            stage: Stage::Gathering,
            shouter: Shouter::new().ok(),
            greeted: true,
        })
    }

    /// Joins the lobby at `address`, as `HOST:PORT`.
    ///
    /// # Errors
    ///
    /// Whatever binding a port or resolving the address says.
    pub(crate) fn join(address: &str, name: &str, game: &str) -> io::Result<Self> {
        let me = guest_id();
        let net = UdpNet::bind(("0.0.0.0", 0), me)?;
        net.connect(HOST, address)?;
        Ok(Self {
            net: Arc::new(net),
            began: None,
            me,
            game: game.to_string(),
            name: name.to_string(),
            seats: 0,
            members: Vec::new(),
            stage: Stage::Gathering,
            shouter: None,
            greeted: false,
        })
    }

    /// This machine's peer in the lobby.
    #[must_use]
    pub(crate) const fn me(&self) -> PeerId {
        self.me
    }

    /// Whether this machine is the host.
    #[must_use]
    pub(crate) fn hosting(&self) -> bool {
        self.me == HOST
    }

    /// Where this machine's lobby socket is: the port a host is joined on.
    #[must_use]
    pub(crate) fn local(&self) -> Option<SocketAddr> {
        self.net.local().ok()
    }

    /// Everyone in, the host first.
    #[must_use]
    pub(crate) fn members(&self) -> &[Member] {
        &self.members
    }

    /// How many seats the session will have, as the host said.
    #[must_use]
    pub(crate) const fn seats(&self) -> u16 {
        self.seats
    }

    /// Where it has got to.
    #[must_use]
    pub(crate) const fn stage(&self) -> &Stage {
        &self.stage
    }

    /// Whether the host could start now: every seat taken and every guest
    /// ready.
    #[must_use]
    pub(crate) fn can_start(&self) -> bool {
        self.stage == Stage::Gathering
            && self.members.len() == usize::from(self.seats)
            && self.members.iter().all(|m| m.ready)
    }

    /// Says whether this guest is ready. Nothing on the host.
    pub(crate) fn set_ready(&mut self, ready: bool) {
        if self.hosting() {
            return;
        }
        self.say(HOST, &Say::Ready { ready });
    }

    /// Starts the session, on the host, if [`can_start`](Self::can_start):
    /// the loop takes it with [`began`](Self::began), sends every guest the
    /// opening's terms and plays it from its next tick. Answers whether it
    /// started.
    pub(crate) fn start(&mut self) -> bool {
        if !self.hosting() || !self.can_start() {
            return false;
        }
        let guests = self
            .members
            .iter()
            .filter(|m| m.peer != HOST)
            .map(|m| m.peer)
            .collect();
        self.began = Some(Started {
            seat: PlayerId(0),
            seats: self.seat_map(),
            width: self.seats,
            terms: None,
            guests,
        });
        self.stage = Stage::Linked;
        true
    }

    /// A session this lobby started, for the loop to play; once.
    pub(crate) fn began(&mut self) -> Option<Started> {
        self.began.take()
    }

    /// The socket, for the session's link.
    pub(crate) fn socket(&self) -> Arc<UdpNet> {
        Arc::clone(&self.net)
    }

    /// Back from a session to gathering: everyone still here, nobody but the
    /// host ready.
    pub(crate) fn back(&mut self) {
        self.stage = Stage::Gathering;
        self.began = None;
        for member in &mut self.members {
            member.ready = member.peer == HOST;
        }
        if self.hosting() {
            self.tell_room();
        }
    }

    /// Tells everyone this machine is going: the host tells every guest, a
    /// guest the host. What is left of the lobby is dropped by the caller.
    pub(crate) fn leave(&self) {
        let everyone: Vec<PeerId> = self
            .members
            .iter()
            .map(|m| m.peer)
            .filter(|peer| *peer != self.me)
            .collect();
        let to = if self.hosting() {
            everyone
        } else {
            std::vec![HOST]
        };
        for peer in to {
            self.say(peer, &Say::Leave);
        }
    }

    /// Reads what has arrived and answers it; a host also shouts on the
    /// local network. Call once a frame.
    pub(crate) fn poll(&mut self) {
        // While linked the session's link reads the socket.
        if self.stage != Stage::Gathering {
            return;
        }
        let net = Arc::clone(&self.net);
        let mut heard: Vec<(PeerId, Say)> = Vec::new();
        let mut joined: Vec<PeerId> = Vec::new();
        let mut lost: Vec<PeerId> = Vec::new();
        net.poll(&mut |from, delivery| match delivery {
            Delivery::Stream {
                channel: Channel::Opening,
                bytes,
            } => {
                if let Ok(said) = corvid_wire::decode::<Say>(bytes) {
                    heard.push((from, said));
                }
            }
            Delivery::Joined => joined.push(from),
            Delivery::Lost { .. } => lost.push(from),
            _ => {}
        });
        if self.hosting() {
            self.host_poll(&heard, &lost);
        } else {
            self.guest_poll(heard, &joined, &lost);
        }
    }

    /// Hears what the session's link read off the socket for the lobby
    /// while linked: a frame, or `None` for a peer lost.
    pub(crate) fn hear(&mut self, from: PeerId, frame: Option<&[u8]>) {
        let heard: Vec<(PeerId, Say)> = frame
            .and_then(|bytes| corvid_wire::decode::<Say>(bytes).ok())
            .map(|said| std::vec![(from, said)])
            .unwrap_or_default();
        let lost: Vec<PeerId> = if frame.is_none() {
            std::vec![from]
        } else {
            Vec::new()
        };
        if self.hosting() {
            self.host_poll(&heard, &lost);
        } else {
            self.guest_poll(heard, &[], &lost);
        }
    }

    fn seat_map(&self) -> BTreeMap<PeerId, PlayerId> {
        self.members.iter().map(|m| (m.peer, m.seat)).collect()
    }

    fn say(&self, to: PeerId, what: &Say) {
        if let Ok(bytes) = corvid_wire::encode(what)
            && let Err(why) = self.net.send_stream(to, Channel::Opening, &bytes)
        {
            tracing::warn!(name: "corvid_app.lobby_unsent", peer = %to, %why, "a lobby frame was not sent");
        }
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
