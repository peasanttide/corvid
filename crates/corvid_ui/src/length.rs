//! Lengths, what one rem is on this display, and the four-sided groups of
//! them a stylesheet writes.
//!
//! Every length resolves once per layout into `I16F16` physical pixels, and
//! everything downstream of that is `I16F16`. A UI rectangle is not a place in
//! the world, so it is not a `GlobalPoint`: `I16F16` reaches ±32 767 px — four
//! times the width of an 8K display — with a 15 µpx step, which is three
//! thousand times finer than a subpixel.

use corvid_fixed::{Factor16, I16F16};
/// A length, in the unit a game writes it in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Length {
    /// Multiples of the root size. The unit a game writes.
    Rem(I16F16),
    /// Physical pixels. What a game writes when it means a hairline.
    Px(I16F16),
    /// A share of the free space along the axis, after the fixed children.
    Fraction(Factor16),
    /// As large as the content, and no larger.
    Auto,
}

impl Length {
    /// No length at all.
    pub const ZERO: Self = Self::Px(I16F16::ZERO);

    /// One rem.
    pub const REM: Self = Self::Rem(I16F16::ONE);

    /// The whole of the free space along the axis.
    pub const FULL: Self = Self::Fraction(Factor16::MAX);

    /// Multiples of the root size — the unit a game writes.
    #[must_use]
    pub const fn rem(multiples: I16F16) -> Self {
        Self::Rem(multiples)
    }

    /// Physical pixels.
    #[must_use]
    pub const fn px(pixels: I16F16) -> Self {
        Self::Px(pixels)
    }

    /// A share of the free space along the axis.
    #[must_use]
    pub const fn fraction(share: Factor16) -> Self {
        Self::Fraction(share)
    }

    /// Whether this length is a share of the free space, which is what the
    /// solver distributes rather than measures.
    #[must_use]
    pub const fn is_fraction(self) -> bool {
        matches!(self, Self::Fraction(_))
    }

    /// The share this length claims, or nothing for the three that claim none.
    #[must_use]
    pub const fn share(self) -> Factor16 {
        match self {
            Self::Fraction(share) => share,
            _ => Factor16::ZERO,
        }
    }

    /// Resolve against a scale and the space available along this axis.
    ///
    /// `Auto` resolves to `content`, which the caller measured. A resolution
    /// that ran past `I16F16` saturates; [`checked_resolve`](Self::checked_resolve)
    /// is what the solver calls, because a layout that saturated silently is
    /// the failure this crate reports as [`TooLarge`](crate::TooLarge).
    ///
    /// ```
    /// use corvid_fixed::{Factor16, I16F16};
    /// use corvid_ui::{Length, Scale};
    ///
    /// let scale = Scale::DEFAULT;
    /// let hundred = I16F16::from_f64(100.0);
    /// let none = I16F16::ZERO;
    ///
    /// // Exact at both ends: no share is nothing, the whole share is all of it.
    /// assert_eq!(Length::Fraction(Factor16::ZERO).resolve(scale, hundred, none), none);
    /// assert_eq!(Length::FULL.resolve(scale, hundred, none), hundred);
    ///
    /// // Sixteen physical pixels to the rem, which is what a designer expects.
    /// assert_eq!(Length::REM.resolve(scale, none, none), I16F16::from_f64(16.0));
    /// ```
    #[must_use]
    pub const fn resolve(self, scale: Scale, free: I16F16, content: I16F16) -> I16F16 {
        match self {
            Self::Rem(multiples) => multiples.saturating_mul(scale.rem),
            Self::Px(pixels) => pixels,
            Self::Fraction(share) => saturating_scale(free, share),
            Self::Auto => content,
        }
    }

    /// Resolve, or nothing when the result ran past what `I16F16` holds.
    #[must_use]
    pub const fn checked_resolve(
        self,
        scale: Scale,
        free: I16F16,
        content: I16F16,
    ) -> Option<I16F16> {
        match self {
            Self::Rem(multiples) => multiples.checked_mul(scale.rem),
            Self::Px(pixels) => Some(pixels),
            Self::Fraction(share) => scale_exactly(free, share),
            Self::Auto => Some(content),
        }
    }
}

impl Default for Length {
    /// [`Auto`](Length::Auto): as large as the content and no larger, which is
    /// what a box that says nothing about its size means.
    fn default() -> Self {
        Self::Auto
    }
}

impl From<Factor16> for Length {
    fn from(share: Factor16) -> Self {
        Self::Fraction(share)
    }
}

/// `value * share`, exactly, or nothing when it ran past `I16F16`.
///
/// Exact at both ends — a share of nothing is nothing and the whole share is
/// the whole value — which a shift by sixteen would not be, because a unit
/// [`Factor16`] is 65 535 rather than 65 536.
pub(crate) const fn scale_exactly(value: I16F16, share: Factor16) -> Option<I16F16> {
    let unit = Factor16::MAX.to_bits() as i64;
    let product = value.to_bits() as i64 * share.to_bits() as i64;
    let half = unit / 2;
    let rounded = if product < 0 {
        product - half
    } else {
        product + half
    } / unit;
    if rounded > i32::MAX as i64 || rounded < i32::MIN as i64 {
        None
    } else {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the two branches above are the range check that makes this narrowing exact"
        )]
        Some(I16F16::from_bits(rounded as i32))
    }
}

/// [`scale_exactly`], saturating rather than answering nothing.
pub(crate) const fn saturating_scale(value: I16F16, share: Factor16) -> I16F16 {
    match scale_exactly(value, share) {
        Some(scaled) => scaled,
        None if value.is_negative() => I16F16::MIN,
        None => I16F16::MAX,
    }
}

/// `total * part / whole`, truncated, for splitting leftover space.
///
/// Called with a running `part` so that the differences between consecutive
/// results sum to exactly `total`: the remainder lands on the last share
/// rather than being spread, which would depend on the order a sum was
/// evaluated in.
pub(crate) const fn split(total: I16F16, part: u32, whole: u32) -> I16F16 {
    if whole == 0 {
        return I16F16::ZERO;
    }
    let scaled = total.to_bits() as i64 * part as i64 / whole as i64;
    if scaled > i32::MAX as i64 {
        I16F16::MAX
    } else if scaled < i32::MIN as i64 {
        I16F16::MIN
    } else {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the two branches above are the range check that makes this narrowing exact"
        )]
        I16F16::from_bits(scaled as i32)
    }
}

/// What one rem is, on this display, this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Scale {
    /// Physical pixels to the rem.
    pub rem: I16F16,
    /// The display's dots to the inch, which [`for_dpi`](Scale::for_dpi)
    /// derives `rem` from and which a game that draws in physical units reads
    /// directly.
    pub dpi: I16F16,
}

impl Scale {
    /// Sixteen physical pixels to the rem at 96 dpi, which is what every other
    /// stack means by a rem and is therefore what a designer expects.
    pub const DEFAULT: Self = Self::for_dpi(I16F16::from_f64(96.0));

    /// A scale from both numbers.
    #[must_use]
    pub const fn new(rem: I16F16, dpi: I16F16) -> Self {
        Self { rem, dpi }
    }

    /// The scale a display of this density asks for: a rem is a sixth of an
    /// inch, which is sixteen pixels at 96 dpi and thirty-two at 192.
    #[must_use]
    pub const fn for_dpi(dpi: I16F16) -> Self {
        Self {
            rem: dpi.saturating_div(I16F16::from_f64(6.0)),
            dpi,
        }
    }
}

impl Default for Scale {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Four lengths, in the order every stylesheet writes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Edges {
    /// The top edge.
    pub top: Length,
    /// The right edge.
    pub right: Length,
    /// The bottom edge.
    pub bottom: Length,
    /// The left edge.
    pub left: Length,
}

impl Edges {
    /// No edges at all.
    pub const NONE: Self = Self::all(Length::ZERO);

    /// Four edges, clockwise from the top.
    #[must_use]
    pub const fn new(top: Length, right: Length, bottom: Length, left: Length) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    /// The same length on all four.
    #[must_use]
    pub const fn all(length: Length) -> Self {
        Self::new(length, length, length, length)
    }

    /// One length top and bottom, another left and right.
    #[must_use]
    pub const fn axes(vertical: Length, horizontal: Length) -> Self {
        Self::new(vertical, horizontal, vertical, horizontal)
    }

    /// The left and right edges resolved and summed.
    ///
    /// An edge is not a share of anything and has no content to be as large
    /// as, so `Fraction` and `Auto` edges resolve to nothing here.
    #[must_use]
    pub fn horizontal(self, scale: Scale) -> Option<I16F16> {
        edge(self.left, scale)?.checked_add(edge(self.right, scale)?)
    }

    /// The top and bottom edges resolved and summed, by
    /// [`horizontal`](Self::horizontal)'s rules.
    #[must_use]
    pub fn vertical(self, scale: Scale) -> Option<I16F16> {
        edge(self.top, scale)?.checked_add(edge(self.bottom, scale)?)
    }
}

/// One edge, resolved against nothing to share and nothing to measure.
pub(crate) const fn edge(length: Length, scale: Scale) -> Option<I16F16> {
    length.checked_resolve(scale, I16F16::ZERO, I16F16::ZERO)
}

impl Default for Edges {
    /// [`NONE`](Edges::NONE), and not four `Auto`s: a box that says nothing
    /// about its padding has none, where a box that says nothing about its
    /// width is as large as its content.
    fn default() -> Self {
        Self::NONE
    }
}

/// A width and a height, each in the unit it was written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Size {
    /// Across.
    pub width: Length,
    /// Down.
    pub height: Length,
}

impl Size {
    /// As large as the content in both directions.
    pub const AUTO: Self = Self::new(Length::Auto, Length::Auto);

    /// Nothing in either direction.
    pub const ZERO: Self = Self::new(Length::ZERO, Length::ZERO);

    /// A size from its two lengths.
    #[must_use]
    pub const fn new(width: Length, height: Length) -> Self {
        Self { width, height }
    }
}

impl Default for Size {
    fn default() -> Self {
        Self::AUTO
    }
}
