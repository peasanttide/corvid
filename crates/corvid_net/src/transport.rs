//! The vocabulary every backend speaks: who is out there, what arrived, and
//! the two ways to send.

use core::fmt;

use corvid_signal::Watch;

/// The largest datagram a transport carries unless it says otherwise.
///
/// 1 200 bytes, which is what a QUIC path that has not probed will carry: the
/// conservative internet MTU less the room a header needs.
pub const DATAGRAM_LIMIT: usize = 1_200;

/// Which machine. Assigned by whatever established the connection; a peer does
/// not choose its own.
///
/// A `PeerId` is not a seat. A machine may hold two seats and a seat may move
/// between machines, so the mapping between this and a game's player is the
/// runtime's business rather than the transport's.
///
/// ```
/// use corvid_net::PeerId;
///
/// assert_eq!(PeerId::from(3).to_string(), "peer 3");
/// assert_eq!(PeerId::NONE.to_string(), "nobody");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerId(pub u16);

impl PeerId {
    /// Nobody. The niche a "not connected" slot uses.
    pub const NONE: Self = Self(u16::MAX);

    /// Whether this is [`NONE`](Self::NONE).
    ///
    /// ```
    /// use corvid_net::PeerId;
    ///
    /// assert!(PeerId::NONE.is_none());
    /// assert!(!PeerId(0).is_none());
    /// ```
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.0 == Self::NONE.0
    }

    /// The number underneath.
    #[must_use]
    pub const fn to_u16(self) -> u16 {
        self.0
    }
}

impl From<u16> for PeerId {
    #[inline]
    fn from(id: u16) -> Self {
        Self(id)
    }
}

impl From<PeerId> for u16 {
    #[inline]
    fn from(peer: PeerId) -> Self {
        peer.0
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            f.write_str("nobody")
        } else {
            write!(f, "peer {}", self.0)
        }
    }
}

/// Which reliable stream. Ordered within a channel and not across them, so a
/// large state transfer does not delay a chat line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Channel {
    /// The opening, and anything that must arrive before the first tick.
    Opening,
    /// A whole `State`, for a join or a resync. The big one.
    Transfer,
    /// Roster changes, ready reports, barrier acknowledgements.
    Control,
    /// The game's own reliable traffic.
    Chat,
}

impl Channel {
    /// Every channel, in declaration order.
    ///
    /// ```
    /// use corvid_net::Channel;
    ///
    /// assert_eq!(Channel::ALL.first(), Some(&Channel::Opening));
    /// ```
    pub const ALL: [Self; 4] = [Self::Opening, Self::Transfer, Self::Control, Self::Chat];

    /// What this channel is called in a report.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Opening => "opening",
            Self::Transfer => "transfer",
            Self::Control => "control",
            Self::Chat => "chat",
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
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

/// Why a peer went away.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Lost {
    /// The other end said goodbye.
    Closed,
    /// It stopped answering.
    TimedOut,
    /// It declined the connection.
    Refused,
    /// The path underneath went away.
    Reset,
}

impl Lost {
    /// What this is called in a report.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::TimedOut => "timed out",
            Self::Refused => "refused",
            Self::Reset => "reset",
        }
    }
}

impl fmt::Display for Lost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Who is here.
///
/// Sorted, in a `Vec`, and hashed over that order — so a roster is a value.
/// Two peers that build one by iterating this build the same one, which a
/// `HashSet` ordered by its hash seed would not give them.
///
/// ```
/// use corvid_net::{PeerId, PeerSet};
///
/// let shuffled: PeerSet = [7, 1, 4, 1].map(PeerId).into_iter().collect();
/// let ordered: PeerSet = [1, 4, 7].map(PeerId).into_iter().collect();
///
/// assert_eq!(shuffled, ordered);
/// assert_eq!(shuffled.iter().map(PeerId::to_u16).collect::<Vec<_>>(), [1, 4, 7]);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerSet {
    /// Sorted and free of duplicates, which every method here maintains.
    peers: Vec<PeerId>,
}

impl PeerSet {
    /// Nobody.
    #[must_use]
    pub const fn new() -> Self {
        Self { peers: Vec::new() }
    }

    /// Adds a peer, and answers whether it was not already there.
    ///
    /// ```
    /// use corvid_net::{PeerId, PeerSet};
    ///
    /// let mut set = PeerSet::new();
    /// assert!(set.insert(PeerId(2)));
    /// assert!(!set.insert(PeerId(2)));
    /// ```
    pub fn insert(&mut self, peer: PeerId) -> bool {
        match self.peers.binary_search(&peer) {
            Ok(_) => false,
            Err(at) => {
                self.peers.insert(at, peer);
                true
            }
        }
    }

    /// Removes a peer, and answers whether it was there.
    pub fn remove(&mut self, peer: PeerId) -> bool {
        match self.peers.binary_search(&peer) {
            Ok(at) => {
                self.peers.remove(at);
                true
            }
            Err(_) => false,
        }
    }

    /// Whether this peer is in the set.
    #[must_use]
    pub fn contains(&self, peer: PeerId) -> bool {
        self.peers.binary_search(&peer).is_ok()
    }

    /// Every peer, in order.
    pub fn iter(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.peers.iter().copied()
    }

    /// The set as a slice, which is already sorted.
    #[must_use]
    pub const fn as_slice(&self) -> &[PeerId] {
        self.peers.as_slice()
    }

    /// How many.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.peers.len()
    }

    /// Whether nobody is here.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }
}

impl FromIterator<PeerId> for PeerSet {
    fn from_iter<I: IntoIterator<Item = PeerId>>(iter: I) -> Self {
        let mut peers: Vec<PeerId> = iter.into_iter().collect();
        peers.sort_unstable();
        peers.dedup();
        Self { peers }
    }
}

impl<'a> IntoIterator for &'a PeerSet {
    type IntoIter = core::iter::Copied<core::slice::Iter<'a, PeerId>>;
    type Item = PeerId;

    fn into_iter(self) -> Self::IntoIter {
        self.peers.iter().copied()
    }
}

impl From<PeerSet> for Vec<PeerId> {
    fn from(set: PeerSet) -> Self {
        set.peers
    }
}

/// Why a send did not happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SendError {
    /// No such peer, or it has gone.
    Unknown(PeerId),
    /// Larger than the path will carry in one datagram. Both numbers are
    /// reported, because a caller that has to split a payload needs the limit
    /// as much as it needs the refusal.
    TooLarge {
        /// What was offered.
        bytes: usize,
        /// What the path will take.
        limit: usize,
    },
    /// The transport is shutting down.
    Closed,
    /// A reliable channel has more waiting to be acknowledged than it will
    /// hold, so this frame was not taken.
    ///
    /// It is not a peer going away and not a path refusing: it is *this* end
    /// declining to queue any more for a far end that has stopped
    /// acknowledging. A caller that has something better to do than send —
    /// coalescing two state transfers into one, or waiting a tick — can; one
    /// that has not may send the same frame again later.
    Backpressure {
        /// How many frames are already waiting.
        waiting: usize,
        /// How many the channel will hold.
        limit: usize,
    },
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unknown(peer) => write!(f, "no route to {peer}"),
            Self::TooLarge { bytes, limit } => {
                write!(f, "a datagram of {bytes} bytes past the limit of {limit}")
            }
            Self::Closed => f.write_str("the transport is closed"),
            Self::Backpressure { waiting, limit } => write!(
                f,
                "{waiting} frames are already waiting to be acknowledged and the limit is {limit}",
            ),
        }
    }
}

impl std::error::Error for SendError {}

/// Bytes to a peer, bytes from a peer, and who is out there.
///
/// It knows nothing about ticks, actions or games: a frame of bytes goes in
/// one end and comes out the other, and what the bytes mean belongs to
/// whatever built them.
///
/// Every method takes `&self`, so a backend carries its own interior
/// mutability. `&mut self` would put a `&mut dyn Transport` on every path that
/// wants to send — including from inside [`poll`](Self::poll), which is the
/// borrow that does not work.
pub trait Transport: Send {
    /// Sends unreliably and unordered: the action stream, where a late packet
    /// is worthless because rollback has already covered for it.
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

    /// Who is here, published rather than returned — a consumer that missed
    /// three changes sees the current set rather than three stale ones.
    fn peers(&self) -> &Watch<PeerSet>;

    /// The largest datagram this transport will carry.
    fn datagram_limit(&self) -> usize {
        DATAGRAM_LIMIT
    }
}
