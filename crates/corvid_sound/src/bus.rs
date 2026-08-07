//! The mixing buses, as a list of edges rather than a walked graph.

use crate::BusId;
use corvid_fixed::Factor16;

/// One mixing bus: a gain, and optionally the bus it feeds.
///
/// Buses are what make "quieter effects, unchanged music" one number rather
/// than a pass over every source. A frame lists them; it does not resolve them,
/// and the effective gain on a source is the backend's arithmetic to do.
///
/// # What is not enforced
///
/// This crate stores the graph and never walks it. Nothing here checks that
/// `parent` names a bus that is present in the same frame, that a source's
/// `bus` does, that the parent chain terminates, or that two entries do not
/// claim the same [`BusId`]. All four are obligations on whoever builds the
/// frame, and a backend has to decide what to do when one is broken — the
/// frame will carry a cycle to it perfectly faithfully.
///
/// ```
/// use corvid_fixed::Factor16;
/// use corvid_sound::{Bus, BusId};
///
/// let effects = Bus::new(BusId(1)).under(BusId::MASTER).with_gain(Factor16::from_f64(0.5));
/// assert_eq!(effects.parent, Some(BusId::MASTER));
///
/// // A bus that names itself as its parent is a cycle, and it is accepted.
/// // Being able to build one is the reason the paragraph above exists.
/// let looped = Bus::new(BusId(2)).under(BusId(2));
/// assert_eq!(looped.parent, Some(BusId(2)));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Bus {
    /// Which bus this is.
    pub id: BusId,
    /// The bus this one feeds, or `None` for a root.
    pub parent: Option<BusId>,
    /// How much of what arrives here leaves it.
    pub gain: Factor16,
}

impl Bus {
    /// A root bus at full gain.
    #[must_use]
    #[inline]
    pub const fn new(id: BusId) -> Self {
        Self {
            id,
            parent: None,
            gain: Factor16::ONE,
        }
    }

    /// Routes this bus into `parent`.
    #[must_use]
    #[inline]
    pub const fn under(self, parent: BusId) -> Self {
        Self {
            parent: Some(parent),
            ..self
        }
    }

    /// Sets the gain.
    #[must_use]
    #[inline]
    pub const fn with_gain(self, gain: Factor16) -> Self {
        Self { gain, ..self }
    }
}

impl Default for Bus {
    /// The master bus at full gain.
    ///
    /// Not derived, because a derived `Factor16` is
    /// [`ZERO`](Factor16::ZERO) and a default bus that is silent is a trap
    /// rather than a neutral starting point.
    #[inline]
    fn default() -> Self {
        Self::new(BusId::MASTER)
    }
}
