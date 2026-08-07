//! One hand, as four values.
//!
//! **A hand here is not a skeleton.** A game that wants twenty-six joints gets
//! them from the runtime directly; what a [`Headset`](crate::Headset) promises
//! is the four things every XR interaction is built from — where the hand is,
//! where it points, how closed it is, and whether it is pinching. Those four
//! are what both scales need, and they are what a recording can carry
//! honestly.

use corvid_fixed::Factor16;
use serde::{Deserialize, Serialize};

use crate::Pose;

/// One hand.
///
/// ```
/// use corvid_xr::Hand;
/// use corvid_fixed::Factor16;
///
/// let open = Hand::default();
/// assert!(!open.is_gripping());
///
/// let fist = Hand { grip: Factor16::MAX, ..open };
/// assert!(fist.is_gripping());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hand {
    /// Where the hand is, for drawing it.
    pub palm: Pose,
    /// Where a ray from the hand goes.
    ///
    /// Not the palm's forward: a runtime aims this at where a person thinks
    /// they are pointing, which is not where their palm faces. At table scale
    /// this is the ray that picks a cell, because the hand itself is far too
    /// coarse to.
    pub aim: Pose,
    /// How closed the fist is.
    pub grip: Factor16,
    /// How closed the pinch is.
    pub pinch: Factor16,
}

impl Hand {
    /// How closed a hand has to be before it counts as closed.
    ///
    /// Half. One number rather than two, and a game that wants hysteresis —
    /// which most do — reads [`grip`](Self::grip) and keeps its own edge.
    pub const CLOSED: Factor16 = Factor16::from_bits(u16::MAX / 2);

    /// A hand at `palm`, aiming the way it faces, open.
    #[must_use]
    #[inline]
    pub const fn open(palm: Pose) -> Self {
        Self {
            palm,
            aim: palm,
            grip: Factor16::MIN,
            pinch: Factor16::MIN,
        }
    }

    /// Whether the fist is closed enough to count as a grab.
    #[must_use]
    #[inline]
    pub const fn is_gripping(self) -> bool {
        self.grip.to_bits() >= Self::CLOSED.to_bits()
    }

    /// Whether the pinch is closed enough to count as one.
    #[must_use]
    #[inline]
    pub const fn is_pinching(self) -> bool {
        self.pinch.to_bits() >= Self::CLOSED.to_bits()
    }

    /// The same hand with the fist closed by `grip`.
    #[must_use]
    #[inline]
    pub const fn gripping(self, grip: Factor16) -> Self {
        Self { grip, ..self }
    }

    /// The same hand with the pinch closed by `pinch`.
    #[must_use]
    #[inline]
    pub const fn pinching(self, pinch: Factor16) -> Self {
        Self { pinch, ..self }
    }

    /// The same hand aiming somewhere other than where the palm faces.
    #[must_use]
    #[inline]
    pub const fn aiming(self, aim: Pose) -> Self {
        Self { aim, ..self }
    }
}
