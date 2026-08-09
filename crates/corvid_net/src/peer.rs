//! Who: a machine's number, a set of them, and one direction between two.

use core::fmt;

use alloc::vec::Vec;

use corvid_macros::id_type;

id_type! {
    /// Which machine. Assigned by whatever established the connection; a peer
    /// does not choose its own.
    ///
    /// Seats are numbered from one. Nought is [`NONE`](Self::NONE) -- nobody --
    /// which is what makes `PeerId::default()` an absent peer rather than a
    /// real one, and what lets a "not connected" slot be a plain `PeerId`
    /// instead of an `Option<PeerId>`.
    ///
    /// A `PeerId` is not a seat in the game's sense. A machine may hold two
    /// seats and a seat may move between machines, so the mapping between this
    /// and a game's player belongs to whatever arranged the session rather than
    /// to the transport.
    ///
    /// ```
    /// use corvid_net::PeerId;
    ///
    /// assert_eq!(PeerId::from(3).to_string(), "PeerId(3)");
    /// assert_eq!(PeerId::default(), PeerId::NONE);
    /// assert!(PeerId::NONE.is_none());
    /// ```
    PeerId, u16, "The number underneath. Nought is nobody."
}

impl PeerId {
    /// Nobody. The niche a "not connected" slot uses, and the number no seat
    /// is given.
    pub const NONE: Self = Self(0);

    /// Whether this is [`NONE`](Self::NONE).
    ///
    /// ```
    /// use corvid_net::PeerId;
    ///
    /// assert!(PeerId::NONE.is_none());
    /// assert!(!PeerId(1).is_none());
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

/// Who is here.
///
/// Sorted, in a `Vec`, and hashed over that order -- so a roster is a value.
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

/// One direction between two peers.
///
/// Directed, and that is the whole of why it is a type: a real link is
/// asymmetric, so the curve a backend applies, the counters it keeps and the
/// state it holds are all per direction. `Link::new(a, b)` and `Link::new(b, a)`
/// are different keys and are meant to be.
///
/// ```
/// use corvid_net::{Link, PeerId};
///
/// let there = Link::new(PeerId(1), PeerId(2));
/// let back = Link::new(PeerId(2), PeerId(1));
///
/// assert_ne!(there, back);
/// assert_eq!(there.reversed(), back);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Link {
    /// Who sent.
    pub from: PeerId,
    /// Who receives.
    pub to: PeerId,
}

impl Link {
    /// The direction from one peer to another.
    #[must_use]
    pub const fn new(from: PeerId, to: PeerId) -> Self {
        Self { from, to }
    }

    /// The other direction.
    #[must_use]
    pub const fn reversed(self) -> Self {
        Self::new(self.to, self.from)
    }
}

impl fmt::Display for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} to {}", self.from, self.to)
    }
}
