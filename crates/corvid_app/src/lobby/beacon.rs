//! Finding a lobby on the local network without typing an address.
//!
//! A host shouts a small datagram at the broadcast address, and at this
//! machine's own loopback, about once a second: which game, whose lobby, the
//! port to join on and how many seats are open. A [`Browser`] listens on the
//! beacon port and keeps a list of who it has heard from lately.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::string::String;
use std::time::{Duration, Instant};
use std::vec::Vec;

use serde::{Deserialize, Serialize};

/// The port lobbies are announced on.
pub(super) const BEACON_PORT: u16 = 47_901;

/// What a beacon datagram starts with, so a stray packet on the port is not
/// read as one.
const MAGIC: &[u8; 4] = b"CVDB";

/// How long a lobby stays listed after its last beacon.
const FORGET: Duration = Duration::from_secs(4);

/// How often a host shouts.
pub(super) const EVERY: Duration = Duration::from_millis(900);

/// What a beacon says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Shout {
    pub(super) game: String,
    pub(super) name: String,
    pub(super) port: u16,
    pub(super) open: u16,
    /// Whether its session is already being played.
    pub(super) running: bool,
}

/// A lobby a [`Browser`] has heard of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Found {
    /// Where to join it: the host's address and its lobby's port.
    pub(crate) address: SocketAddr,
    /// The host's name.
    pub(crate) name: String,
    /// How many seats are still open.
    pub(crate) open: u16,
    /// Whether its session is already being played.
    pub(crate) running: bool,
    /// When it was last heard from.
    heard: Instant,
}

/// The socket a host shouts from.
#[derive(Debug)]
pub(super) struct Shouter {
    socket: UdpSocket,
    last: Option<Instant>,
}

impl Shouter {
    pub(super) fn new() -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        socket.set_broadcast(true)?;
        socket.set_nonblocking(true)?;
        Ok(Self { socket, last: None })
    }

    /// Shouts, if it is time to.
    pub(super) fn shout(&mut self, what: &Shout) {
        if self.last.is_some_and(|last| last.elapsed() < EVERY) {
            return;
        }
        self.last = Some(Instant::now());
        let Ok(body) = corvid_wire::encode(what) else {
            return;
        };
        let mut datagram = MAGIC.to_vec();
        datagram.extend_from_slice(&body);
        for to in [Ipv4Addr::BROADCAST, Ipv4Addr::LOCALHOST] {
            // A network with no broadcast route refuses the first; the second
            // still finds a browser on this machine. Neither is worth more
            // than a debug line.
            if let Err(why) = self.socket.send_to(&datagram, (to, BEACON_PORT)) {
                tracing::debug!(name: "corvid_app.beacon_unsent", %to, %why, "a lobby beacon was not sent");
            }
        }
    }
}

/// A listener for lobbies on the local network, for one game.
#[derive(Debug)]
pub(crate) struct Browser {
    socket: UdpSocket,
    game: String,
    found: Vec<Found>,
}

impl Browser {
    /// Listens for `game`'s lobbies on [`BEACON_PORT`].
    ///
    /// # Errors
    ///
    /// Whatever binding the port says -- most often that another program on
    /// this machine is already listening on it.
    pub(crate) fn new(game: &str) -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, BEACON_PORT))?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            game: game.into(),
            found: Vec::new(),
        })
    }

    /// Reads whatever beacons have arrived and forgets lobbies gone quiet.
    pub(crate) fn poll(&mut self) {
        let mut buffer = [0_u8; 1_200];
        while let Ok((length, from)) = self.socket.recv_from(&mut buffer) {
            let Some(body) = buffer.get(..length).and_then(|b| b.strip_prefix(MAGIC)) else {
                continue;
            };
            let Ok(shout) = corvid_wire::decode::<Shout>(body) else {
                continue;
            };
            if shout.game != self.game {
                continue;
            }
            let address = SocketAddr::new(from.ip(), shout.port);
            let heard = Instant::now();
            let found = Found {
                address,
                name: shout.name,
                open: shout.open,
                running: shout.running,
                heard,
            };
            match self.found.iter_mut().find(|f| f.address == address) {
                Some(have) => *have = found,
                None => self.found.push(found),
            }
        }
        self.found.retain(|f| f.heard.elapsed() < FORGET);
    }

    /// The lobbies heard from lately, in the order they were first heard.
    #[must_use]
    pub(crate) fn found(&self) -> &[Found] {
        &self.found
    }
}
