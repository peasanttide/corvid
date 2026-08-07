//! What a window is doing, as state rather than as events.

/// How many physical pixels wide and tall something is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Size {
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
}

impl Size {
    /// A size from its two numbers.
    #[must_use]
    #[inline]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Whether either dimension is zero, which is what a minimised window
    /// reports on most platforms.
    #[must_use]
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Everything about the window that a renderer or a game might want to know.
///
/// # Why this is published rather than handed over
///
/// A resize is *state*, not an event: the latest size is the only one that
/// matters, and a renderer that processed every intermediate size during a drag
/// would reconfigure its surface fifty times to arrive where one reconfigure
/// would have put it. So this goes into a [`corvid_signal`] cell, which keeps
/// the latest value and drops the rest, and whoever cares reads it once per
/// frame.
///
/// The other half of the reason is the direction. A callback reaching from the
/// event loop into the game is the shape in which a window ends up able to run
/// a tick, and a window that can run a tick is a window that can change what a
/// session computes. Publishing state is a shape in which it cannot: nothing
/// here calls a game, and the game reads this when it is ready.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct SurfaceState {
    /// How big the drawable area is, in physical pixels.
    pub size: Size,
    /// How many physical pixels the platform puts in a logical one, times a
    /// thousand.
    ///
    /// An integer, because a scale factor that arrived as an `f64` would be a
    /// different number on a machine that parsed it differently and because
    /// nothing downstream needs more than three decimal places of it. A
    /// hundred-and-fifty-per-cent display reads `1500`.
    pub scale_milli: u32,
    /// Whether the window has keyboard focus.
    ///
    /// A window that loses focus stops hearing key releases, which is why
    /// `corvid_input`'s `Devices::released_all` is called when this goes false.
    pub focused: bool,
    /// Whether the platform says the window is not visible.
    ///
    /// A minimised window, or one entirely behind another. A frame drawn into
    /// one is work nobody sees.
    pub occluded: bool,
}

impl Default for SurfaceState {
    /// A window of no size on an unscaled display, focused and not occluded.
    ///
    /// Hand-written for one field: a derived `Default` puts `0` in
    /// `scale_milli`, which is not a scale factor at all — every conversion
    /// through it either collapses a dimension to nothing or divides by zero —
    /// and `#[non_exhaustive]` makes this the only way a crate outside this one
    /// can build the type, so the derived value would be the *only* one they
    /// could name. The neutral factor is `1000`, which is what
    /// [`scale_from`](Self::scale_from) answers for a platform that reported
    /// nothing usable.
    fn default() -> Self {
        Self {
            size: Size::new(0, 0),
            scale_milli: 1_000,
            focused: false,
            occluded: false,
        }
    }
}

impl SurfaceState {
    /// The scale factor a platform reported, as this type stores it.
    ///
    /// Saturating, so a platform reporting something absurd pins rather than
    /// wrapping, and a negative or `NaN` factor becomes one.
    #[must_use]
    pub fn scale_from(factor: f64) -> u32 {
        if factor.is_finite() && factor > 0.0 {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "the branch this is in has established that the value is finite and positive, and the `min` above the cast keeps it inside a u32; the cast is the rounding this type is defined to do"
            )]
            let scaled = (factor * 1000.0).min(f64::from(u32::MAX)) as u32;
            scaled
        } else {
            1000
        }
    }
}
