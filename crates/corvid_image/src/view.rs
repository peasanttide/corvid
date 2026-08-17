//! What the viewer can see, and what that makes a tile worth.

use crate::{Source, SourceId};

/// A rectangle of one source's texture space, in uv.
///
/// Floats, because this is the client ring and a viewport is a float: nothing
/// here is hashed, nothing here crosses the wire, and two machines are allowed
/// to disagree about it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UvRect {
    /// The lower corner, `[u, v]`.
    pub min: [f32; 2],
    /// The upper corner, `[u, v]`.
    pub max: [f32; 2],
}

impl UvRect {
    /// The whole of a source.
    pub const FULL: Self = Self {
        min: [0.0, 0.0],
        max: [1.0, 1.0],
    };

    /// A rectangle from its corners.
    #[must_use]
    pub const fn new(min: [f32; 2], max: [f32; 2]) -> Self {
        Self { min, max }
    }

    /// This rectangle clipped to `[0, 1]` on both axes, with a `NaN` corner
    /// read as the edge it is nearest.
    ///
    /// Every consumer of a view calls this first. A rectangle that reaches off
    /// the edge of a plate is the normal case rather than an error -- a map
    /// half off the side of the screen is still on the screen -- and clipping
    /// it here is what keeps the tile enumeration from asking for tiles the
    /// source does not have.
    #[must_use]
    pub fn clipped(self) -> Self {
        // `f32::clamp` answers `NaN` for a `NaN` input, so the comparisons are
        // written out: an unclamped `NaN` would reach `as u32` and become zero
        // silently, which is a rectangle at the origin rather than a refusal.
        const fn hold(value: f32, fallback: f32) -> f32 {
            if value > 0.0 {
                if value < 1.0 { value } else { 1.0 }
            } else if value <= 0.0 {
                0.0
            } else {
                fallback
            }
        }
        Self {
            min: [hold(self.min[0], 0.0), hold(self.min[1], 0.0)],
            max: [hold(self.max[0], 1.0), hold(self.max[1], 1.0)],
        }
    }

    /// Whether this rectangle covers nothing.
    ///
    /// A rectangle with a `NaN` corner covers nothing, which is the same answer
    /// [`clipped`](Self::clipped) would arrive at from the other direction.
    #[must_use]
    pub fn is_empty(self) -> bool {
        let wide = self.max[0] > self.min[0];
        let tall = self.max[1] > self.min[1];
        !(wide && tall)
    }
}

impl Default for UvRect {
    fn default() -> Self {
        Self::FULL
    }
}

/// One source as the viewer sees it this frame.
///
/// Three numbers, and between them they are the whole of what residency is
/// planned against: where on the source the eye is, how finely it is being
/// magnified there, and how much the frame would suffer if it were wrong.
/// Computing them is the caller's job, because the caller is the one holding a
/// camera and this crate deliberately holds none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceView {
    /// Which source.
    pub source: SourceId,
    /// The part of it on screen.
    pub visible: UvRect,
    /// How many source texels one screen pixel covers.
    ///
    /// One means the plate is at its native resolution, and larger means it is
    /// minified -- four texels to a pixel is level two, and drawing it from
    /// level zero would cost sixteen times the memory for a picture nobody can
    /// tell apart. Below one the plate is magnified and level zero is already
    /// the finest thing there is.
    pub texels_per_pixel: f32,
    /// How much this source matters, from zero to one.
    ///
    /// The active map is one. A layer fading out, a plate under a lens the
    /// player is not looking through, a minimap: less. Zero asks for nothing
    /// beyond the root tile that keeps it drawable.
    pub weight: f32,
}

impl SourceView {
    /// A view of the whole of a source at its native resolution, at full
    /// weight.
    #[must_use]
    pub const fn full(source: SourceId) -> Self {
        Self {
            source,
            visible: UvRect::FULL,
            texels_per_pixel: 1.0,
            weight: 1.0,
        }
    }

    /// The finest zoom worth streaming for this view, clamped to what the
    /// source has.
    ///
    /// `floor(log2(texels_per_pixel))`, which is the level whose texels are at
    /// most one screen pixel each. A `NaN` reads as one texel per pixel, so a
    /// caller that divides by a zero viewport gets level zero rather than an
    /// unordered plan.
    ///
    /// ```
    /// use corvid_image::{PixelFormat, Source, SourceId, SourceView, TileConfig, extent};
    ///
    /// let config = TileConfig::MIN_SPEC;
    /// let plate = Source::new(&config, extent(16384, 16384), PixelFormat::SRGB8)?;
    ///
    /// let mut view = SourceView::full(SourceId(0));
    /// assert_eq!(view.level(&plate), 0);
    /// view.texels_per_pixel = 5.0;
    /// assert_eq!(view.level(&plate), 2);
    /// // And it never asks for a zoom past the top of the pyramid.
    /// view.texels_per_pixel = 1.0e9;
    /// assert_eq!(view.level(&plate), plate.top_level());
    /// # Ok::<(), corvid_image::TileError>(())
    /// ```
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the value is clamped into 1..=65536 first, and a float cast in Rust saturates rather than wrapping, so a NaN lands on 1"
    )]
    pub fn level(&self, source: &Source) -> u8 {
        let ratio = self.texels_per_pixel;
        let ratio = if ratio > 1.0 {
            if ratio < 65536.0 { ratio } else { 65536.0 }
        } else {
            1.0
        };
        let level = (ratio as u32).max(1).ilog2();
        let top = u32::from(source.top_level());
        if level > top {
            source.top_level()
        } else {
            level as u8
        }
    }

    /// The weight as the sixteen-bit rank a [`Priority`] compares by.
    ///
    /// Quantised rather than compared as a float, and that is not an
    /// optimisation. `f32` has no total order, so a plan sorted by raw weights
    /// would be at the mercy of whichever `NaN` reached a comparator first --
    /// and a plan that is not a function of its input is the bug this crate
    /// exists to not have.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the value is clamped into 0..=65535 first, and a float cast in Rust saturates rather than wrapping, so a NaN lands on 0"
    )]
    pub fn rank(&self) -> u16 {
        let weight = self.weight;
        let weight = if weight > 0.0 {
            if weight < 1.0 { weight } else { 1.0 }
        } else {
            0.0
        };
        (weight * 65535.0) as u16
    }
}

/// Whether a tile is one the source cannot be drawn at all without.
///
/// Ordered so that `Root` outranks `Detail`, which is the whole of its
/// purpose: one tile per visible source is reserved above everything else, so a
/// second source cannot be starved to nothing by a first one with a large
/// working set. Something coarse and correct beats a hole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// A tile that sharpens a source already drawable without it.
    Detail,
    /// The top of a visible source's pyramid.
    Root,
}

/// What a tile is worth this frame. Greater is more valuable.
///
/// The field order is the comparison order and each field is oriented so that
/// larger means keep: the root tiers first, then the viewer's own weighting,
/// then the level -- coarser before finer, so that within one source the
/// fallback a blurry frame is made of always outranks the detail that would
/// sharpen it. That last one is what turns an overloaded budget into a soft
/// picture instead of a hole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Priority {
    /// Root before detail.
    pub tier: Tier,
    /// [`SourceView::rank`], so a heavier view wins.
    pub weight: u16,
    /// The zoom, coarser first.
    pub level: u8,
}

impl Priority {
    /// The value of a tile at `level` under a view of `rank`.
    #[must_use]
    pub const fn new(tier: Tier, weight: u16, level: u8) -> Self {
        Self {
            tier,
            weight,
            level,
        }
    }
}
