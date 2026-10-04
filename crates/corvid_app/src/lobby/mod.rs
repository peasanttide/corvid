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
use std::net::{Ipv4Addr, SocketAddr};
use std::string::{String, ToString};
use std::sync::Arc;
use std::vec::Vec;

use corvid_behavior::PlayerId;
use corvid_net::{Channel, Delivery, PeerId, Transport as _};
use corvid_net_udp::UdpNet;

mod beacon;
mod bind;
mod guest;
mod host;
mod say;
mod shared;

pub(crate) use beacon::Browser;
pub(crate) use shared::{Shared, announce};

use beacon::Shouter;
use bind::guest_id;
pub(crate) use bind::here_for;
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
    /// The seats nobody took, which the host plays with the game's bot.
    /// Empty on a guest.
    pub(crate) bots: Vec<PlayerId>,
    /// Whether this machine joins a session already being played, and so
    /// waits for a state before it plays.
    pub(crate) joining: bool,
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
    /// On a guest: the readiness it last told the host, until a room says
    /// otherwise; asked again, it says nothing (see `set_ready`).
    told: Option<bool>,
    /// On a host while linked: the terms its session started on, for a
    /// machine that joins it in progress.
    terms: Option<Vec<u8>>,
    /// On a host while linked: the seats its bot plays, which a machine can
    /// join.
    open: Vec<PlayerId>,
    /// Machines let into the session in progress, to be told so once the
    /// room has been.
    joining: Vec<(PeerId, Say)>,
}

impl Lobby {
    /// Hosts a lobby for `game` on `port` -- 0 for any free one -- with
    /// `seats` seats, the first of them this machine's. A `local` lobby is
    /// bound to the loopback address and shouts nothing: only this machine
    /// can join it, and it touches no network.
    ///
    /// # Errors
    ///
    /// Whatever binding the port says.
    pub(crate) fn host(
        port: u16,
        name: &str,
        game: &str,
        seats: u16,
        local: bool,
    ) -> io::Result<Self> {
        let at = if local {
            Ipv4Addr::LOCALHOST
        } else {
            Ipv4Addr::UNSPECIFIED
        };
        let net = UdpNet::bind((at, port), HOST)?;
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
            shouter: if local { None } else { Shouter::new().ok() },
            greeted: true,
            told: None,
            terms: None,
            open: Vec::new(),
            joining: Vec::new(),
        })
    }

    /// Joins the lobby at `address`, as `HOST:PORT`: from the loopback
    /// address when it is on this machine's, so a lobby on this machine is
    /// joined without touching the network.
    ///
    /// # Errors
    ///
    /// Whatever binding a port or resolving the address says.
    pub(crate) fn join(address: &str, name: &str, game: &str) -> io::Result<Self> {
        let me = guest_id();
        let net = UdpNet::bind((here_for(address)?, 0), me)?;
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
            told: None,
            terms: None,
            open: Vec::new(),
            joining: Vec::new(),
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

    /// Whether the host could start now: every guest ready. A seat nobody
    /// took is played by the host's bot, and a machine can join it later.
    #[must_use]
    pub(crate) fn can_start(&self) -> bool {
        self.stage == Stage::Gathering && self.members.iter().all(|m| m.ready)
    }

    /// Says whether this guest is ready, unless it said so last and no room
    /// has said otherwise: asked every frame, copies would fill the window
    /// and the host's answers its own, refusing a room. Nothing on the host.
    pub(crate) fn set_ready(&mut self, ready: bool) {
        if self.hosting() || self.told == Some(ready) {
            return;
        }
        self.told = Some(ready);
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
        self.open = (0..self.seats)
            .map(PlayerId)
            .filter(|seat| self.members.iter().all(|m| m.seat != *seat))
            .collect();
        self.began = Some(Started {
            seat: PlayerId(0),
            seats: self.seat_map(),
            width: self.seats,
            terms: None,
            guests,
            bots: self.open.clone(),
            joining: false,
        });
        self.stage = Stage::Linked;
        true
    }

    /// Keeps the terms a host's session started on, for a machine that
    /// joins it in progress.
    pub(crate) fn keep_terms(&mut self, terms: Vec<u8>) {
        self.terms = Some(terms);
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
        self.terms = None;
        self.told = None;
        self.open.clear();
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
