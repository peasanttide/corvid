//! The vocabulary every backend speaks: who is out there, what arrived, and
//! the two ways to send.

use corvid_macros::named_enum;
use corvid_signal::Watch;
use thiserror::Error;

use crate::peer::{PeerId, PeerSet};

/// The largest datagram a transport carries unless it says otherwise.
///
/// 1 200 bytes, which is what a QUIC path that has not probed will carry: the
/// conservative internet MTU less the room a header needs.
pub const DATAGRAM_LIMIT: usize = 1_200;

named_enum! {
    /// Which reliable stream. Ordered within a channel and not across them, so
    /// a large state transfer does not delay a chat line.
    ///
    /// ```
    /// use corvid_net::Channel;
    ///
    /// assert_eq!(Channel::ALL.first(), Some(&Channel::Opening));
    /// assert_eq!(Channel::Transfer.to_string(), "transfer");
    /// ```
    #[non_exhaustive]
    Channel {
        /// The opening, and anything that must arrive before the first tick.
        Opening = "opening",
        /// A whole `State`, for a join or a resync. The big one.
        Transfer = "transfer",
        /// Roster changes, ready reports, barrier acknowledgements.
        Control = "control",
        /// The game's own reliable traffic.
        Chat = "chat",
    }
}

/// What arrived.
///
/// Borrowed rather than owned, because the bytes belong to the transport for
/// the length of the call and a handler that wants to keep them says so by
/// copying them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Delivery<'a> {
    /// Unreliable, unordered, possibly duplicated.
    Datagram(&'a [u8]),
    /// Reliable and ordered within its channel.
    Stream {
        /// Which stream it arrived on.
        channel: Channel,
        /// The frame, exactly as it was sent.
        bytes: &'a [u8],
    },
    /// This peer is now reachable. It precedes everything else from that peer.
    Joined,
    /// It is not, and will not be without a new connection.
    Lost {
        /// Why.
        because: Lost,
    },
}

impl<'a> Delivery<'a> {
    /// The payload, for the two variants that carry one.
    ///
    /// ```
    /// use corvid_net::Delivery;
    ///
    /// assert_eq!(Delivery::Datagram(b"tick").bytes(), Some(&b"tick"[..]));
    /// assert_eq!(Delivery::Joined.bytes(), None);
    /// ```
    #[must_use]
    pub const fn bytes(self) -> Option<&'a [u8]> {
        match self {
            Self::Datagram(bytes) | Self::Stream { bytes, .. } => Some(bytes),
            Self::Joined | Self::Lost { .. } => None,
        }
    }
}

named_enum! {
    /// Why a peer went away.
    ///
    /// ```
    /// use corvid_net::Lost;
    ///
    /// assert_eq!(Lost::TimedOut.to_string(), "timed out");
    /// ```
    #[non_exhaustive]
    Lost {
        /// The other end said goodbye.
        Closed = "closed",
        /// It stopped answering.
        TimedOut = "timed out",
        /// It declined the connection.
        Refused = "refused",
        /// The path underneath went away.
        Reset = "reset",
    }
}

/// Why a send did not happen.
///
/// [`Display`](core::fmt::Display) and [`Error`](core::error::Error) come from
/// `thiserror`, so each message sits on the variant it belongs to rather than
/// in a `match` arm three screens away that a new variant does not have to
/// update.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Error)]
#[non_exhaustive]
pub enum SendError {
    /// No such peer, or it has gone.
    #[error("no route to {0}")]
    Unknown(PeerId),
    /// Larger than the path will carry in one datagram. Both numbers are
    /// reported, because a caller that has to split a payload needs the limit
    /// as much as it needs the refusal.
    #[error("a datagram of {bytes} bytes past the limit of {limit}")]
    TooLarge {
        /// What was offered.
        bytes: usize,
        /// What the path will take.
        limit: usize,
    },
    /// The transport is shutting down.
    #[error("the transport is closed")]
    Closed,
    /// A reliable channel has more waiting to be acknowledged than it will
    /// hold, so this frame was not taken.
    ///
    /// It is not a peer going away and not a path refusing: it is *this* end
    /// declining to queue any more for a far end that has stopped
    /// acknowledging. A caller that has something better to do than send --
    /// coalescing two state transfers into one, or waiting a tick -- can; one
    /// that has not may send the same frame again later.
    #[error("{waiting} frames are already waiting to be acknowledged and the limit is {limit}")]
    Backpressure {
        /// How many frames are already waiting.
        waiting: usize,
        /// How many the channel will hold.
        limit: usize,
    },
}

/// Bytes to a peer, bytes from a peer, and who is out there.
///
/// It knows nothing about ticks, actions or games: a frame of bytes goes in
/// one end and comes out the other, and what the bytes mean belongs to
/// whatever built them.
///
/// Every method takes `&self`, so a backend carries its own interior
/// mutability. `&mut self` would put a `&mut dyn Transport` on every path that
/// wants to send -- including from inside [`poll`](Self::poll), which is the
/// borrow that does not work.
///
/// [`Debug`] for the reason `corvid_time::Elapsed` is: a transport is held behind
/// a `Box<dyn Transport>` by a caller that derives its own, and a trait object
/// prints only what its trait allows. What a backend owes is a name and its own
/// bookkeeping, not the traffic through it.
pub trait Transport: Send + core::fmt::Debug {
    /// Sends unreliably and unordered: the action stream, where a late packet
    /// is worthless because a peer rolling back has already covered for it.
    ///
    /// # Errors
    ///
    /// [`SendError::Unknown`] if `to` is not a peer this transport can reach,
    /// [`SendError::TooLarge`] if `bytes` is past
    /// [`datagram_limit`](Self::datagram_limit), and [`SendError::Closed`] if
    /// the transport is shutting down.
    fn send_datagram(&self, to: PeerId, bytes: &[u8]) -> Result<(), SendError>;

    /// Sends reliably and in order within `channel`: joins, state transfers,
    /// the opening, chat.
    ///
    /// # Errors
    ///
    /// [`SendError::Unknown`] if `to` is not a peer this transport can reach,
    /// [`SendError::Backpressure`] if the channel already holds more than it
    /// will, and [`SendError::Closed`] if the transport is shutting down. A
    /// stream frame is never [`TooLarge`](SendError::TooLarge): the transport
    /// splits it.
    fn send_stream(&self, to: PeerId, channel: Channel, bytes: &[u8]) -> Result<(), SendError>;

    /// Hands everything that has arrived to `sink`, oldest first.
    ///
    /// The sink is a `&mut dyn FnMut` rather than a returned iterator because
    /// an iterator would borrow the transport for the length of the loop, and a
    /// peer that wants to answer a packet inside the loop could not.
    fn poll(&self, sink: &mut dyn FnMut(PeerId, Delivery<'_>));

    /// Who is here, published rather than returned -- a consumer that missed
    /// three changes sees the current set rather than three stale ones.
    ///
    /// A transport may publish to this once and never again, so nothing may
    /// depend on a further publication to make progress. In particular a
    /// thread that must be able to exit cannot park on
    /// [`Watch::blocking_wait`] and rely on a roster change to wake it:
    /// [`Offline`](crate::Offline) never has one, and a backend whose last
    /// peer has already gone may not either.
    fn peers(&self) -> &Watch<PeerSet>;

    /// The largest datagram this transport will carry.
    fn datagram_limit(&self) -> usize {
        DATAGRAM_LIMIT
    }
}
