//! Playing over a transport: the peer, the wire, and the tick that joins them.
//!
//! Everything here is behind the `net` feature, and everything a game sees of
//! it is one builder call. [`App::transport`](crate::App::transport) hands over
//! a `Box<dyn Transport>`; from there the loop owns a
//! [`Peer`](corvid_lockstep::Peer) and this module is what it drives per tick.
//! `State` and `Present` are untouched -- a game that plays over a network
//! and a game that does not are the same two implementations, which is the
//! claim the whole lockstep design exists to support.

use std::{collections::BTreeMap, vec::Vec};

use corvid_behavior::{PlayerId, State};
use corvid_lockstep::{Budget, Halt, Peer};
use corvid_net::{PeerId, Transport};
use corvid_replay::Refused;
use corvid_replay::Session;
use corvid_time::Tick;

mod agree;
mod play;
mod rescue;
mod tell;
mod traffic;

pub use traffic::{Departures, TickTraffic, Traffic};

use traffic::{Control, Transfer};

/// A peer, the transport its datagrams ride on, and the seat map between them.
///
/// One tick is [`play`](Self::play): submit this machine's action, fold in
/// whatever arrived, simulate, and send. The order is the one
/// `corvid_lockstep`'s own documentation gives, and the reason it is that order
/// is that the action for `now + delay` should be in the datagram this tick
/// sends rather than in the next one.
pub(crate) struct Link<S: State> {
    /// This machine's whole lockstep state, which owns the session.
    peer: Peer<S>,
    /// What carries a datagram to the other machines.
    transport: Box<dyn Transport>,
    /// What arrived this tick, copied out of the transport's borrow so that
    /// folding one in can send an answer.
    ///
    /// A field rather than a local so that the allocation is made once for the
    /// run rather than once per tick.
    inbox: Vec<Vec<u8>>,
    /// The last datagram this peer built, encoded. Also once per run.
    outbound: Vec<u8>,
    /// What the last tick did.
    traffic: TickTraffic,
    /// And what all of them did.
    totals: Traffic,
    /// Who has said what about which seat leaving.
    ///
    /// Held beside the peer rather than in it because it is about the
    /// *machines* and not about the session: what reaches the session is the
    /// one agreed tick, through [`Peer::depart`](corvid_lockstep::Peer::depart).
    departures: Departures,
    /// This machine's own proposal for each seat, kept so that it can be said
    /// again to a peer that arrives late or missed it.
    mine: BTreeMap<PlayerId, Tick>,
    /// Somewhere to leave the game's caches while a rescue borrows the peer.
    ///
    /// `Scratch` is `Default` and nothing else, so this is the one way to hand
    /// a peer method something to simulate with from a context that is not
    /// holding the run's own. It is a memo either way -- a tick may not read
    /// anything out of one that its arguments do not imply -- so a fresh one is
    /// always a correct one.
    /// The newest tick any peer has said it has actions up to.
    ///
    /// **This is what decides whether a stall is survivable.** A peer is stuck
    /// when the rows it is waiting for are older than the oldest row anybody
    /// still sends -- and a datagram's window reaches back
    /// [`CATCHUP`](corvid_lockstep::CATCHUP) rows from its sender's head, so
    /// the comparison is this against the tick every seat has been confirmed
    /// to.
    ///
    /// It is measured in *ticks of the session* rather than in tries, which the
    /// first version of this was: a peer that is briefly ahead declines to
    /// simulate on every pass of the loop, and a loop with a `Fake` clock
    /// passes thousands of times a millisecond -- so a counter of "how often did
    /// I decline" reached any threshold in microseconds of ordinary play and
    /// asked for a state transfer in the middle of a healthy session.
    heard_head: Tick,
}

impl<S: State> Link<S> {
    /// A link over `transport`, playing `seat` of `session`.
    pub(crate) fn new(
        session: Session<S>,
        seat: PlayerId,
        budget: Budget,
        transport: Box<dyn Transport>,
    ) -> Self {
        let seats = session.log.players();
        Self {
            peer: Peer::new(session, seat, budget),
            transport,
            inbox: Vec::new(),
            outbound: Vec::new(),
            traffic: TickTraffic::default(),
            totals: Traffic::default(),
            departures: Departures::new(seats),
            mine: BTreeMap::new(),
            heard_head: Tick::ZERO,
        }
    }

    /// The session being played, which the peer owns.
    pub(crate) const fn session(&self) -> &Session<S> {
        &self.peer.session
    }

    /// The same, mutably, for the two things the loop does to a session that
    /// have nothing to do with the network: writing a save out of it and
    /// letting it forget its far past.
    pub(crate) const fn session_mut(&mut self) -> &mut Session<S> {
        &mut self.peer.session
    }

    /// The session, once the run is over and the peer is finished with it.
    pub(crate) fn into_session(self) -> Session<S> {
        self.peer.session
    }

    /// The state this peer is at.
    pub(crate) const fn state(&self) -> &S {
        self.peer.state()
    }

    /// The tick that state is at.
    pub(crate) const fn tick(&self) -> Tick {
        self.peer.tick()
    }

    /// What the last tick did.
    pub(crate) const fn traffic(&self) -> TickTraffic {
        self.traffic
    }

    /// What the whole run has done.
    pub(crate) const fn totals(&self) -> Traffic {
        self.totals
    }

    /// Opens on a state that came from somewhere other than the opening: a save
    /// slot, a recorded session, or another machine.
    ///
    /// # Errors
    ///
    /// [`Error::Halted`](crate::Error::Halted) for a tick outside the session
    /// the peer is holding.
    pub(crate) fn adopt(&mut self, at: Tick, state: S) -> Result<(), crate::Error> {
        self.peer.adopt(at, state).map_err(halted)
    }
}

/// A seat map with nothing in it yet.
///
/// [`PeerId(n)`](corvid_net::PeerId) plays [`PlayerId(n - 1)`](PlayerId), which
/// is what two peers started by the same command line have. The subtraction is
/// the two crates' own conventions meeting: a seat is counted from nought and a
/// peer from one, because [`PeerId(0)`](corvid_net::PeerId::NONE) is nobody.
///
/// It is a placeholder: a session assembled by a lobby is told who is in which
/// seat, and that mapping arrives over
/// [`Channel::Control`](corvid_net::Channel) with the roster rather than being
/// inferred from a connection's order.
///
/// ```
/// use corvid_app::seat_of;
/// use corvid_behavior::PlayerId;
/// use corvid_net::PeerId;
///
/// assert_eq!(seat_of(PeerId(1)), PlayerId(0));
/// assert_eq!(seat_of(PeerId(2)), PlayerId(1));
///
/// // Saturating rather than wrapping, so the function is total. Nobody is not
/// // a sender -- a transport delivers the peer a datagram really came from --
/// // so what this answers for one is a number and not a claim.
/// assert_eq!(seat_of(PeerId::NONE), PlayerId(0));
/// ```
#[must_use]
pub const fn seat_of(peer: PeerId) -> PlayerId {
    PlayerId(peer.0.saturating_sub(1))
}

/// The other half of [`seat_of`], for the one caller that has a seat and needs
/// the peer that plays it.
///
/// ```
/// use corvid_app::{peer_of, seat_of};
/// use corvid_behavior::PlayerId;
///
/// assert_eq!(seat_of(peer_of(PlayerId(0))), PlayerId(0));
/// assert_eq!(seat_of(peer_of(PlayerId(41))), PlayerId(41));
/// ```
#[must_use]
pub const fn peer_of(seat: PlayerId) -> PeerId {
    PeerId(seat.0.saturating_add(1))
}

/// The socket `--listen PORT --connect HOST:PORT` asks for.
///
/// Every address on this machine, on `port`, announcing this machine as the
/// [`PeerId`] its own seat maps to under [`peer_of`] -- which is what
/// [`seat_of`] reads back on the other end, and is why two processes started by
/// one command line need nothing else told to them. `peer` is the other
/// machine, as `HOST:PORT`.
///
/// The seat it announces is **this machine's own seat number**, so the two
/// halves of a two-machine session are `--seat 0 --listen A --connect B` and
/// `--seat 1 --listen B --connect A`. The peer it connects to is the other of
/// those two, which is the whole of the arithmetic a command line can do: a
/// session with more machines in it is assembled by a lobby over
/// [`Channel::Control`](corvid_net::Channel), which is told who sits where
/// rather than working it out from a subtraction.
///
/// That is why a seat above one is
/// [`Argument::Pairing`](crate::Argument::Pairing) rather than something to
/// compute. There is no third address here to connect to, and the seat at the
/// far end of the one address there is would be `1 - seat`, which for seat two
/// is not a seat at all -- the link would come up, carry datagrams and match no
/// seat at the other end.
///
/// # Errors
///
/// [`Error::Argument`](crate::Error::Argument) carrying
/// [`Argument::Pairing`](crate::Argument::Pairing) for a seat this pair of
/// flags cannot arrange -- a command line that could not be acted on, which is
/// what [`main`](crate::main) writes to stderr -- and
/// [`Error::Socket`](crate::Error::Socket) if the port will not bind or the
/// other machine's address will not resolve. The second pair are facts about
/// the machine and the network rather than about what was typed, which is why
/// they are one variant naming which of the two it was.
pub(crate) fn udp(
    port: u16,
    seat: PlayerId,
    peer: &str,
) -> Result<Box<dyn Transport>, crate::Error> {
    // Before the socket, so that a session nobody could have joined does not
    // bind a port on the way to saying so.
    let Some(other) = 1_u16.checked_sub(seat.0) else {
        return Err(crate::Error::Argument(crate::Argument::Pairing { seat }));
    };
    let here = ("0.0.0.0", port);
    let socket =
        corvid_net_udp::UdpNet::bind(here, peer_of(seat)).map_err(|why| crate::Error::Socket {
            what: "bind",
            address: format!("0.0.0.0:{port}"),
            why,
        })?;
    socket
        .connect(peer_of(PlayerId(other)), peer)
        .map_err(|why| crate::Error::Socket {
            what: "reach",
            address: peer.to_owned(),
            why,
        })?;
    Ok(Box::new(socket))
}

/// The log refusing this machine's own action.
pub(super) const fn refused(why: Refused) -> crate::Error {
    crate::Error::Log(why)
}

/// A peer that cannot carry on, sorted into the two things that means.
pub(super) fn halted(why: Halt) -> crate::Error {
    match why {
        Halt::Desync(desync) => crate::Error::Diverged(Box::new(desync)),
        other => crate::Error::Halted(Box::new(other)),
    }
}

impl<S: State> core::fmt::Debug for Link<S> {
    /// The peer and the counters. Not the transport, which is a trait object
    /// with no `Debug` bound, and not the session, which is the run.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Link")
            .field("peer", &self.peer)
            .field("traffic", &self.traffic)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a failed assertion in a test is a failed test, which is what a test is for"
    )]

    use super::udp;
    use crate::{Argument, Error};
    use corvid_behavior::PlayerId;

    #[test]
    fn a_seat_two_command_lines_cannot_arrange_is_refused_before_a_port_is_bound() {
        // The address is one nothing is listening on and the port is one this
        // never gets as far as binding: the refusal happens on the seat, which
        // is the whole assertion. A run that computed `1 - 2` instead would
        // connect to seat 65535 and match no seat at the far end.
        let why = udp(0, PlayerId(2), "127.0.0.1:1").expect_err("seat two is not one of a pair");
        let Error::Argument(why) = why else {
            panic!("a seat a command line cannot arrange is a command line: {why:?}");
        };
        assert_eq!(why, Argument::Pairing { seat: PlayerId(2) });
        // And it says so with the seat in it, so an operator knows which flag
        // to change.
        assert!(why.to_string().contains("seat 2"), "{why}");
    }
}
