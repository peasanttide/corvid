//! The three things one frame of input is made of.
//!
//! Split from [`Input`](crate::Input) because a file stays under 400 lines, and
//! this is the seam that was already there: a snapshot is a container, and
//! these are what it contains. Nothing here knows about an action set, an
//! identifier or a device -- they are the values, and the snapshot is the
//! answer to "which one".

use corvid_fixed::Signed16;

/// One on-or-off action, with the two edges around it.
///
/// `held` is the level and the other two are edges: `pressed` on the frame the
/// action went down, `released` on the frame it came up. A game that only wants
/// the level reads `held`, and a game that wants "this frame, once" reads
/// `pressed` and does not have to remember last frame's answer.
///
/// No combination is rejected, and one that looks wrong is not. A tap that
/// starts and finishes inside one frame arrives as `pressed` and `released`
/// with `held` false, which is the honest report of what happened and is
/// exactly the event a game must not miss. Producing the edges is the job of
/// whatever fills the snapshot -- a device layer, or a test -- and this type only
/// carries them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Digital {
    /// Whether the action is down now.
    pub held: bool,
    /// Whether it went down between the last frame and this one.
    pub pressed: bool,
    /// Whether it came up between the last frame and this one.
    pub released: bool,
}

impl Digital {
    /// Down, with no edge -- the steady state of a held button.
    pub const HELD: Self = Self {
        held: true,
        pressed: false,
        released: false,
    };

    /// Up, with no edge.
    ///
    /// This is what a query about an action outside the active set answers
    /// with, and what [`Default`] gives.
    pub const RELEASED: Self = Self {
        held: false,
        pressed: false,
        released: false,
    };
}

/// One two-axis action, read either as a deflection or as a displacement.
///
/// Both axes are [`Signed16`], which covers `-1.0 ..= 1.0` exactly and is
/// integer storage -- a stick position that reached a game as `f32` would be a
/// different number on a machine that rounded differently, and the whole point
/// of this crate is to be the last thing between a device and a deterministic
/// tick.
///
/// A one-axis action is one of these with `y` at zero; there is no separate
/// type for it, because a trigger and a stick differ in what they are bound to
/// rather than in what they carry.
///
/// **What one of these means depends on which accessor it came out of**, and
/// that is the whole of why there are two. [`Input::analog`](crate::Input::analog) answers a
/// *deflection*: how far a control is pushed, which is a rate, and which the
/// frame's `dt` multiplies. [`Input::delta`](crate::Input::delta) answers a *displacement*: how far
/// something moved during the frame, which is a quantity already proportional
/// to how long the frame lasted, and which `dt` must not multiply again. The
/// type is the same because the storage is; the two are never mixed in one
/// slot, because a binding fills one accessor or the other and never both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Analog {
    /// The horizontal axis, positive to the right.
    pub x: Signed16,
    /// The vertical axis, positive up.
    pub y: Signed16,
}

impl Analog {
    /// Centred.
    ///
    /// This is what a query about an action outside the active set answers
    /// with, and what [`Default`] gives.
    pub const ZERO: Self = Self {
        x: Signed16::ZERO,
        y: Signed16::ZERO,
    };

    /// An analog value from the two axes.
    #[must_use]
    #[inline]
    pub const fn new(x: Signed16, y: Signed16) -> Self {
        Self { x, y }
    }
}

/// The rectangle a pointer is reported against, in physical pixels.
///
/// This is here rather than in a window crate because it is the other half of
/// [`Input::pointer`](crate::Input::pointer): a pointer arrives in the window's own normalised
/// coordinates, and the only thing that turns one back into pixels is the size
/// of the thing it was normalised against. A game handed the first without the
/// second can tell which button the pointer is nearest and cannot tell how many
/// pixels wide that button is.
///
/// Physical pixels, so a game that lays its interface out in them lays it out
/// at the size the display actually has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Viewport {
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
}

impl Viewport {
    /// A viewport from its two numbers.
    #[must_use]
    #[inline]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Whether either dimension is zero, which is what a minimised window
    /// reports and what no layout can be solved in.
    #[must_use]
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}
