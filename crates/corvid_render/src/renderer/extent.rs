//! How big a target is, and how eagerly finished frames are handed over.

/// How many pixels wide and tall a target is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Extent {
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
}

impl Extent {
    /// An extent from its two numbers.
    #[must_use]
    #[inline]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Whether either dimension is zero, which is what a minimised window
    /// reports and what nothing can be drawn into.
    #[must_use]
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Width over height, or one for an extent with no height.
    ///
    /// What a projection wants, and the reason it is here rather than in the
    /// game: a zero-height target has no aspect ratio, and the answer that
    /// draws nothing is better than the infinity that spreads through a matrix.
    #[must_use]
    #[inline]
    #[expect(
        clippy::cast_precision_loss,
        reason = "a viewport is at most 65535 pixels on any target this workspace builds for, and an f32 counts integers exactly to 16.7 million"
    )]
    pub fn aspect(self) -> f32 {
        if self.height == 0 {
            1.0
        } else {
            self.width as f32 / self.height as f32
        }
    }

    /// The same extent with neither dimension below one.
    ///
    /// A texture of zero width cannot be created, and a surface of zero width
    /// cannot be configured, so this is what a minimised window is stored as.
    #[must_use]
    #[inline]
    pub(super) const fn at_least_one(self) -> Self {
        Self {
            width: if self.width == 0 { 1 } else { self.width },
            height: if self.height == 0 { 1 } else { self.height },
        }
    }
}

/// How eagerly a windowed renderer hands finished frames over.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Pacing {
    /// Wait for the display. Every frame is shown, none is torn, and the loop
    /// runs at the refresh rate.
    #[default]
    Display,
    /// Do not wait. The loop runs as fast as it can and the display shows
    /// whatever was finished when it looked, which tears and is what a latency
    /// measurement wants.
    Immediate,
}
