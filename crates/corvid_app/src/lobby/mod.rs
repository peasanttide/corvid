//! Gathering machines into a session before it starts.
//!
//! One machine hosts: it opens a [`Lobby`] on a port, and shouts about it on
//! the local network. Others join it, by the address a [`Browser`] found or by
//! one typed in. The host seats everyone as they arrive, each guest says when
//! it is ready, and the host starts the session. Starting sends every guest
//! the terms of the opening the host is playing from, and hands every
//! machine's socket to
//! its own loop, which from its next tick plays that opening over it in
//! lockstep -- whatever the loop was playing alone before.
//!
//! A lobby is driven by the game, from its controller, on the loop's thread:
//! [`poll`](Lobby::poll) it once a frame and draw what it says. Everything it
//! does is client-local; nothing in it reaches a digest.

use std::collections::BTreeMap;
use std::io;
use std::net::SocketAddr;
use std::string::{String, ToString};
use std::vec::Vec;

use corvid_behavior::PlayerId;
use corvid_net::{Channel, Delivery, PeerId, Transport};
use corvid_net_udp::UdpNet;

mod beacon;
mod guest;
pub(crate) mod handoff;
mod host;
mod say;

pub use beacon::{BEACON_PORT, Browser, Found};

use beacon::Shouter;
use handoff::Started;
use say::Say;

/// The peer a host always is.
const HOST: PeerId = PeerId(1);

/// One machine in a lobby.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// Which machine.
    pub peer: PeerId,
    /// What it calls itself.
    pub name: String,
    /// The seat it will play.
    pub seat: PlayerId,
    /// Whether it is ready to start. The host always is.
    pub ready: bool,
    address: Option<String>,
}

/// Where a lobby has got to.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Stage {
    /// Waiting for people, or for the host.
    Gathering,
    /// The host turned this machine away, and said why.
    Refused(String),
    /// The session has started, and the loop is playing it.
    Started,
    /// The host went away before starting.
    Closed,
}

/// A lobby, hosted here or joined.
#[derive(Debug)]
pub struct Lobby {
    /// The socket, until the session starts and the loop takes it.
    net: Option<UdpNet>,
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
    pub fn host(port: u16, name: &str, game: &str, seats: u16) -> io::Result<Self> {
        let net = UdpNet::bind(("0.0.0.0", port), HOST)?;
        let host = Member {
            peer: HOST,
            name: name.to_string(),
            seat: PlayerId(0),
            ready: true,
            address: None,
        };
        Ok(Self {
            net: Some(net),
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
    pub fn join(address: &str, name: &str, game: &str) -> io::Result<Self> {
        let me = guest_id();
        let net = UdpNet::bind(("0.0.0.0", 0), me)?;
        net.connect(HOST, address)?;
        Ok(Self {
            net: Some(net),
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

    /// Whether this machine is the host.
    #[must_use]
    pub fn hosting(&self) -> bool {
        self.me == HOST
    }

    /// Where this machine's lobby socket is: the port a host is joined on.
    #[must_use]
    pub fn local(&self) -> Option<SocketAddr> {
        self.net.as_ref().and_then(|net| net.local().ok())
    }

    /// Everyone in, the host first.
    #[must_use]
    pub fn members(&self) -> &[Member] {
        &self.members
    }

    /// How many seats the session will have, as the host said.
    #[must_use]
    pub const fn seats(&self) -> u16 {
        self.seats
    }

    /// Where it has got to.
    #[must_use]
    pub const fn stage(&self) -> &Stage {
        &self.stage
    }

    /// Whether this machine is ready. The host always is.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.members
            .iter()
            .find(|m| m.peer == self.me)
            .is_some_and(|m| m.ready)
    }

    /// Whether the host could start now: every seat taken and every guest
    /// ready.
    #[must_use]
    pub fn can_start(&self) -> bool {
        self.stage == Stage::Gathering
            && self.members.len() == usize::from(self.seats)
            && self.members.iter().all(|m| m.ready)
    }

    /// Says whether this guest is ready. Nothing on the host.
    pub fn set_ready(&mut self, ready: bool) {
        if self.hosting() {
            return;
        }
        self.say(HOST, &Say::Ready { ready });
    }

    /// Starts the session, on the host, if [`can_start`](Self::can_start):
    /// this machine's loop sends every guest the opening's terms and plays it from
    /// its next tick. Answers whether it started.
    pub fn start(&mut self) -> bool {
        if !self.hosting() || !self.can_start() {
            return false;
        }
        let Some(net) = self.net.take() else {
            return false;
        };
        let guests = self
            .members
            .iter()
            .filter(|m| m.peer != HOST)
            .map(|m| m.peer)
            .collect();
        handoff::leave(Started {
            transport: Box::new(net),
            seat: PlayerId(0),
            seats: self.seat_map(),
            width: self.seats,
            terms: None,
            guests,
        });
        self.stage = Stage::Started;
        true
    }

    /// Reads what has arrived and answers it; a host also shouts on the
    /// local network. Call once a frame.
    pub fn poll(&mut self) {
        let Some(net) = self.net.as_ref() else {
            return;
        };
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

    fn seat_map(&self) -> BTreeMap<PeerId, PlayerId> {
        self.members.iter().map(|m| (m.peer, m.seat)).collect()
    }

    fn say(&self, to: PeerId, what: &Say) {
        let Some(net) = self.net.as_ref() else {
            return;
        };
        if let Ok(bytes) = corvid_wire::encode(what)
            && let Err(why) = net.send_stream(to, Channel::Opening, &bytes)
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
