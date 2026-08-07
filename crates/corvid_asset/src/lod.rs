//! How coarse a resident asset is.

use core::fmt;

/// How coarse a resident asset is.
///
/// Zero is the finest, because every mesh pipeline in the world numbers them
/// that way, and [`PLACEHOLDER`](Self::PLACEHOLDER) is the coarsest value a
/// `u8` has. So the derived order runs from detailed to crude and a promotion
/// is a *decrease*:
///
/// ```
/// use corvid_asset::Lod;
///
/// assert!(Lod::FINEST < Lod(2));
/// assert!(Lod(2) < Lod::PLACEHOLDER);
/// assert_eq!(Lod(2).finer(), Some(Lod(1)));
/// assert_eq!(Lod::FINEST.finer(), None);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lod(
    /// The level. Zero is the finest.
    pub u8,
);

impl Lod {
    /// The finest. Zero, because every mesh pipeline in the world numbers them
    /// this way.
    pub const FINEST: Self = Self(0);

    /// The placeholder: present, cheap, and never what a screenshot wants.
    pub const PLACEHOLDER: Self = Self(u8::MAX);

    /// Whether this is the placeholder rather than anything that was decoded.
    #[must_use]
    #[inline]
    pub const fn is_placeholder(self) -> bool {
        self.0 == Self::PLACEHOLDER.0
    }

    /// The next finer level, or [`None`] at [`FINEST`](Self::FINEST).
    #[must_use]
    #[inline]
    pub const fn finer(self) -> Option<Self> {
        match self.0.checked_sub(1) {
            Some(level) => Some(Self(level)),
            None => None,
        }
    }

    /// The coarsest level of a chain `levels` long, or [`None`] for a chain
    /// with no levels in it.
    ///
    /// ```
    /// use corvid_asset::Lod;
    ///
    /// assert_eq!(Lod::coarsest(3), Some(Lod(2)));
    /// assert_eq!(Lod::coarsest(1), Some(Lod::FINEST));
    /// assert_eq!(Lod::coarsest(0), None);
    /// ```
    #[must_use]
    #[inline]
    pub const fn coarsest(levels: u8) -> Option<Self> {
        match levels.checked_sub(1) {
            Some(level) => Some(Self(level)),
            None => None,
        }
    }
}

impl Default for Lod {
    /// [`PLACEHOLDER`](Self::PLACEHOLDER), because a slot that has decoded
    /// nothing is answering with the placeholder.
    #[inline]
    fn default() -> Self {
        Self::PLACEHOLDER
    }
}

impl From<u8> for Lod {
    #[inline]
    fn from(level: u8) -> Self {
        Self(level)
    }
}

impl From<Lod> for u8 {
    #[inline]
    fn from(lod: Lod) -> Self {
        lod.0
    }
}

impl fmt::Display for Lod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_placeholder() {
            f.write_str("placeholder")
        } else {
            write!(f, "lod {}", self.0)
        }
    }
}
