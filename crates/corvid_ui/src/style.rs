//! Everything the solver reads off a node, and the constants a game writes.
//!
//! A [`Style`] is `const`-constructible all the way down, so a game writes
//! `const TITLE: Style = ...` and its styles are values the program has before it
//! runs rather than values it builds every frame.

use crate::length::{Edges, Length, Size};
use corvid_color::Rgba8;
use corvid_fixed::I16F16;

/// Which way a container lays its children out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Left to right.
    Row,
    /// Top to bottom. What a menu is.
    #[default]
    Column,
}

impl Axis {
    /// The other one.
    #[must_use]
    pub const fn across(self) -> Self {
        match self {
            Self::Row => Self::Column,
            Self::Column => Self::Row,
        }
    }

    /// Whether this axis runs horizontally.
    #[must_use]
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Self::Row)
    }
}

/// Where the leftover space along the axis goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Justify {
    /// All of it after the last child.
    #[default]
    Start,
    /// Half before the first child and half after the last.
    Centre,
    /// All of it before the first child.
    End,
    /// Shared between the children, and none at the ends.
    Between,
    /// Shared around the children, so the ends get half a share each.
    Around,
}

/// What a child does with the space across the axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Align {
    /// Against the near edge.
    #[default]
    Start,
    /// In the middle.
    Centre,
    /// Against the far edge.
    End,
    /// As wide as the container, unless the child said how wide it is.
    Stretch,
}

/// How text in a node is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextStyle {
    /// The em size, resolved like any other length.
    pub size: Length,
    /// Extra space between the lines of a wrapped label, on top of the font's
    /// own line height.
    pub leading: I16F16,
}

impl TextStyle {
    /// One rem, and no extra leading.
    pub const DEFAULT: Self = Self {
        size: Length::REM,
        leading: I16F16::ZERO,
    };

    /// Text at a size, with no extra leading.
    #[must_use]
    pub const fn new(size: Length) -> Self {
        Self {
            size,
            leading: I16F16::ZERO,
        }
    }
}

/// [`DEFAULT`](TextStyle::DEFAULT).
///
/// Hand-written because a derived `Default` is zeroes, and text set at no size
/// with no leading is text nobody can read.
impl Default for TextStyle {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Everything the solver reads off a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Style {
    /// How wide, before `min` and `max`.
    pub width: Length,
    /// How tall, before `min` and `max`.
    pub height: Length,
    /// The floor on the resolved size. `Auto` is no floor.
    pub min: Size,
    /// The ceiling on the resolved size. `Auto` is no ceiling.
    pub max: Size,
    /// Inside the border, around the content.
    pub padding: Edges,
    /// Outside the border, between this box and its siblings.
    pub margin: Edges,
    /// Between the children, along the axis.
    pub gap: Length,
    /// Which way the children go.
    pub axis: Axis,
    /// Where the leftover space along the axis goes.
    pub justify: Justify,
    /// What the children do with the space across the axis.
    pub align: Align,
    /// What fills the box.
    pub background: Rgba8,
    /// What text in the box is drawn with.
    pub foreground: Rgba8,
    /// The corner radius.
    pub corner: Length,
    /// How thick the border is. Zero draws none.
    pub border: Length,
    /// What the border is drawn with.
    pub border_colour: Rgba8,
    /// How text in the box is set.
    pub text: TextStyle,
    /// Whether the focus may land here.
    pub focusable: bool,
    /// Whether the children are scissored to this box's content, which costs
    /// one more draw in the renderer.
    pub clip: bool,
}

impl Style {
    /// A box that says nothing: as large as its content, transparent, and not
    /// focusable.
    pub const DEFAULT: Self = Self {
        width: Length::Auto,
        height: Length::Auto,
        min: Size::AUTO,
        max: Size::AUTO,
        padding: Edges::NONE,
        margin: Edges::NONE,
        gap: Length::ZERO,
        axis: Axis::Column,
        justify: Justify::Start,
        align: Align::Start,
        background: Rgba8::TRANSPARENT,
        foreground: Rgba8::WHITE,
        corner: Length::ZERO,
        border: Length::ZERO,
        border_colour: Rgba8::TRANSPARENT,
        text: TextStyle::DEFAULT,
        focusable: false,
        clip: false,
    };

    /// [`DEFAULT`](Self::DEFAULT), spelled as a call so a `const` chain starts
    /// the way a builder does.
    #[must_use]
    pub const fn new() -> Self {
        Self::DEFAULT
    }

    /// The same style, this wide.
    #[must_use]
    pub const fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// The same style, this tall.
    #[must_use]
    pub const fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// The same style, with this floor on its resolved size.
    #[must_use]
    pub const fn min(mut self, min: Size) -> Self {
        self.min = min;
        self
    }

    /// The same style, with this ceiling on its resolved size.
    #[must_use]
    pub const fn max(mut self, max: Size) -> Self {
        self.max = max;
        self
    }

    /// The same style, with this padding.
    #[must_use]
    pub const fn padding(mut self, padding: Edges) -> Self {
        self.padding = padding;
        self
    }

    /// The same style, with this margin.
    #[must_use]
    pub const fn margin(mut self, margin: Edges) -> Self {
        self.margin = margin;
        self
    }

    /// The same style, with this much between its children.
    #[must_use]
    pub const fn gap(mut self, gap: Length) -> Self {
        self.gap = gap;
        self
    }

    /// The same style, laying its children out this way.
    #[must_use]
    pub const fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    /// The same style, putting the leftover space here.
    #[must_use]
    pub const fn justify(mut self, justify: Justify) -> Self {
        self.justify = justify;
        self
    }

    /// The same style, aligning its children this way across the axis.
    #[must_use]
    pub const fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// The same style, filled with this.
    #[must_use]
    pub const fn background(mut self, colour: Rgba8) -> Self {
        self.background = colour;
        self
    }

    /// The same style, with text drawn in this.
    #[must_use]
    pub const fn foreground(mut self, colour: Rgba8) -> Self {
        self.foreground = colour;
        self
    }

    /// The same style, with corners this round.
    #[must_use]
    pub const fn corner(mut self, corner: Length) -> Self {
        self.corner = corner;
        self
    }

    /// The same style, with a border this thick in this colour.
    #[must_use]
    pub const fn border(mut self, width: Length, colour: Rgba8) -> Self {
        self.border = width;
        self.border_colour = colour;
        self
    }

    /// The same style, with text set this way.
    #[must_use]
    pub const fn text(mut self, text: TextStyle) -> Self {
        self.text = text;
        self
    }

    /// The same style, with the focus allowed here or not.
    #[must_use]
    pub const fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// The same style, scissoring its children to its content box or not.
    #[must_use]
    pub const fn clip(mut self, clip: bool) -> Self {
        self.clip = clip;
        self
    }
}

/// [`DEFAULT`](Style::DEFAULT).
///
/// Hand-written for the same reason [`TextStyle`]'s is: the zeroes a derive
/// would produce are not a style a node can be laid out with.
impl Default for Style {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A heading: two rems of text, and a rem of air under it.
pub const TITLE: Style = Style::new()
    .text(TextStyle::new(Length::rem(I16F16::from_f64(2.0))))
    .margin(Edges::new(
        Length::ZERO,
        Length::ZERO,
        Length::REM,
        Length::ZERO,
    ));

/// What [`button`](crate::button) is styled with: focusable, padded, rounded,
/// and filled.
pub const BUTTON: Style = Style::new()
    .axis(Axis::Row)
    .align(Align::Centre)
    .justify(Justify::Centre)
    .padding(Edges::axes(
        Length::rem(I16F16::from_f64(0.25)),
        Length::rem(I16F16::from_f64(0.75)),
    ))
    .corner(Length::rem(I16F16::from_f64(0.25)))
    .background(Rgba8::hex(0x1E_29_3B_FF))
    .foreground(Rgba8::hex(0xE2_E8_F0_FF))
    .focusable(true);

/// A surface to put widgets on: padded, rounded, and dark.
pub const PANEL: Style = Style::new()
    .padding(Edges::all(Length::REM))
    .gap(Length::rem(I16F16::from_f64(0.5)))
    .corner(Length::rem(I16F16::from_f64(0.5)))
    .background(Rgba8::hex(0x0F_17_2A_FF));
