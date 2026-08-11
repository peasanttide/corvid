//! What the mouse pointer is doing, and what a game would like it to do.

/// Whether the pointer is visible, and whether it may leave the window.
///
/// A game asks for one of these through `Controller::cursor`, once per displayed
/// frame; the platform applies it and reports back through
/// [`Input::cursor`](crate::Input::cursor). It is **client-local in the strong
/// sense**: it never enters a tick, never goes on the wire and never reaches a
/// digest, for the same reason a camera does not. Two peers whose pointers are
/// in different modes still agree about the game.
///
/// # The four, and which one a game wants
///
/// | | Visible | Leaves the window | For |
/// |---|---|---|---|
/// | [`Free`](Self::Free) | yes | yes | menus, a desktop strategy game, anything with a real cursor |
/// | [`Hidden`](Self::Hidden) | no | yes | a game drawing its own cursor, or a cutscene |
/// | [`Confined`](Self::Confined) | yes | no | a windowed game with edge-scrolling, or a drag that must not escape |
/// | [`Locked`](Self::Locked) | no | no | first-person look, and an orbit camera being dragged |
///
/// [`Locked`](Self::Locked) is the one that matters and the one worth being
/// precise about: it pins the pointer where it is and the platform reports
/// **relative motion only**. That is what
/// [`Input::delta`](crate::Input::delta) already answers with, so a camera
/// written against `delta` needs no change when the mode does -- it simply stops
/// running out of screen. A game that reads
/// [`Input::pointer`](crate::Input::pointer) while locked is reading a position
/// that no longer moves.
///
/// # The platform may say no
///
/// Pointer locking is a permission on some platforms and a protocol extension
/// on others: a browser grants it only from a user gesture, Wayland needs the
/// pointer-constraints protocol, and a compositor may simply decline. So this
/// is a **request**, and what actually happened is
/// [`Input::cursor`](crate::Input::cursor) -- which a game should read rather
/// than assume, because the failure mode of assuming is a camera that turns at
/// the speed of a mouse hitting the edge of a monitor.
///
/// The runtime falls back rather than failing: a refused
/// [`Locked`](Self::Locked) becomes [`Confined`](Self::Confined), and a refused
/// [`Confined`](Self::Confined) becomes [`Free`](Self::Free). Visibility is not
/// a permission anywhere, so the hiding half of a request always takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[non_exhaustive]
pub enum Cursor {
    /// Visible, and free to leave the window.
    ///
    /// The default, because it is what a window does when nobody has said
    /// otherwise and because it is the mode a player can always get out of. A
    /// game that locks the pointer and then panics leaves the desktop usable.
    #[default]
    Free,
    /// Invisible, and free to leave the window.
    Hidden,
    /// Visible, and kept inside the window.
    Confined,
    /// Invisible and pinned in place; motion is reported as displacement only.
    Locked,
}

impl Cursor {
    /// Whether the pointer is drawn.
    #[must_use]
    #[inline]
    pub const fn is_visible(self) -> bool {
        matches!(self, Self::Free | Self::Confined)
    }

    /// Whether the pointer is kept inside the window.
    #[must_use]
    #[inline]
    pub const fn is_grabbed(self) -> bool {
        matches!(self, Self::Confined | Self::Locked)
    }

    /// Whether the pointer is pinned, so only displacement is reported.
    ///
    /// The one a camera asks about: while this is true,
    /// [`Input::pointer`](crate::Input::pointer) stops moving and
    /// [`Input::delta`](crate::Input::delta) is the whole of what the mouse
    /// says.
    #[must_use]
    #[inline]
    pub const fn is_locked(self) -> bool {
        matches!(self, Self::Locked)
    }

    /// The mode to try when this one is refused, or [`None`] for
    /// [`Free`](Self::Free), which no platform refuses.
    ///
    /// `Locked -> Confined -> Free`, and `Hidden -> Free`. The runtime walks this
    /// so that a game asking for a lock it cannot have still gets the strongest
    /// thing the platform will give it, rather than nothing.
    #[must_use]
    #[inline]
    pub const fn fallback(self) -> Option<Self> {
        match self {
            Self::Free => None,
            Self::Locked => Some(Self::Confined),
            Self::Hidden | Self::Confined => Some(Self::Free),
        }
    }
}
