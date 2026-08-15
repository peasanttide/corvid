//! Whether the wearer can see the room.

use serde::{Deserialize, Serialize};

/// Passthrough, and whether there is any.
///
/// Three values rather than a `bool` and an `Option<bool>`, because "this
/// headset cannot show you the room" is an answer a game acts on -- it hides the
/// toggle -- and is not a failure to report one.
///
/// ```
/// use corvid_xr::Passthrough;
///
/// assert_eq!(Passthrough::from(true), Passthrough::On);
/// assert_eq!(bool::try_from(Passthrough::Unavailable), Err(Passthrough::Unavailable));
/// ```
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Passthrough {
    /// This headset does not offer it.
    #[default]
    Unavailable,
    /// It offers it, and it is off.
    Off,
    /// It offers it, and it is on.
    On,
}

impl Passthrough {
    /// Whether the wearer can see the room.
    #[must_use]
    #[inline]
    pub const fn is_on(self) -> bool {
        matches!(self, Self::On)
    }

    /// Whether asking would achieve anything.
    #[must_use]
    #[inline]
    pub const fn is_available(self) -> bool {
        !matches!(self, Self::Unavailable)
    }

    /// The state this becomes when asked for `on`, given what is available.
    ///
    /// [`Unavailable`](Self::Unavailable) stays unavailable, which is what
    /// makes asking safe.
    #[must_use]
    #[inline]
    pub const fn asked(self, on: bool) -> Self {
        if self.is_available() {
            if on { Self::On } else { Self::Off }
        } else {
            Self::Unavailable
        }
    }
}

impl From<bool> for Passthrough {
    /// An available passthrough, on or off.
    #[inline]
    fn from(on: bool) -> Self {
        if on { Self::On } else { Self::Off }
    }
}

impl TryFrom<Passthrough> for bool {
    type Error = Passthrough;

    /// # Errors
    ///
    /// [`Passthrough::Unavailable`], which is not a yes or a no.
    #[inline]
    fn try_from(passthrough: Passthrough) -> Result<Self, Self::Error> {
        match passthrough {
            Passthrough::Unavailable => Err(Passthrough::Unavailable),
            Passthrough::Off => Ok(false),
            Passthrough::On => Ok(true),
        }
    }
}
