//! What a node can be, and the free functions a game builds one with.
//!
//! A builder is a free function rather than a type per widget, so `.child()`
//! chains without a `::new()` at every level and a second type per widget buys
//! nothing. The chain reads the same either way.

use crate::{
    element::Element,
    focus::Signal,
    style::{self, Style},
    text::Line,
};
use corvid_fixed::Factor16;

/// A short bounded string, so a label costs no allocation and digests to a
/// fixed encoding on every target.
pub type Text = Line;

/// What a node is.
///
/// A closed set, because a game that needs a new widget wants a composition of
/// these rather than a trait object in the arena -- an arena of trait objects
/// is an arena that cannot be hashed, cloned, or compared, which is three of
/// the four things this crate does with one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Kind<I> {
    /// Lays its children out along [`Style::axis`].
    #[default]
    Container,
    /// Text, measured through [`Metrics`](crate::Metrics).
    Label {
        /// What it says.
        text: Text,
        /// Whether it breaks to fit the width its style gives it. A label
        /// whose width is `Auto` or a share has no width to break against and
        /// does not wrap.
        wrap: bool,
    },
    /// A container that raises an intent when it is signalled.
    Button {
        /// Which signal raises it.
        on: Signal,
        /// What it raises.
        intent: I,
    },
    /// A value in `0..=1` a player drags or nudges.
    Slider {
        /// Where it is.
        value: Factor16,
        /// How far one nudge moves it. Zero is a slider that cannot be nudged.
        step: Factor16,
        /// What it raises when it changes.
        intent: I,
    },
    /// On or off.
    Toggle {
        /// Which.
        on: bool,
        /// What it raises when it changes.
        intent: I,
    },
    /// Space with no content, so [`Justify::Between`](crate::Justify::Between)
    /// has something to push.
    Spacer,
}

impl<I> Kind<I> {
    /// The intent this node raises, if it raises one.
    #[must_use]
    pub const fn intent(&self) -> Option<&I> {
        match self {
            Self::Button { intent, .. }
            | Self::Slider { intent, .. }
            | Self::Toggle { intent, .. } => Some(intent),
            Self::Container | Self::Label { .. } | Self::Spacer => None,
        }
    }
}

/// A container that stacks its children downwards.
#[must_use]
pub const fn column<I>() -> Element<I> {
    Element::new(Kind::Container, Style::DEFAULT)
}

/// A container that lays its children out left to right.
#[must_use]
pub const fn row<I>() -> Element<I> {
    Element::new(
        Kind::Container,
        Style::DEFAULT.axis(crate::style::Axis::Row),
    )
}

/// Text, cut at [`Line::CAPACITY`](crate::Line::CAPACITY) if it is longer.
#[must_use]
pub const fn label<I>(text: &str) -> Element<I> {
    Element::new(
        Kind::Label {
            text: Line::truncated(text),
            wrap: false,
        },
        Style::DEFAULT,
    )
}

/// Text that breaks to fit the width its style gives it.
#[must_use]
pub const fn paragraph<I>(text: &str) -> Element<I> {
    Element::new(
        Kind::Label {
            text: Line::truncated(text),
            wrap: true,
        },
        Style::DEFAULT,
    )
}

/// A button with a label in it, focusable, raising `intent` when it is
/// activated.
#[must_use]
pub fn button<I>(text: &str, intent: I) -> Element<I> {
    Element::new(
        Kind::Button {
            on: Signal::Activate,
            intent,
        },
        style::BUTTON,
    )
    .child(label(text))
}

/// A slider at `value`, nudged a sixteenth at a time.
#[must_use]
pub const fn slider<I>(value: Factor16, intent: I) -> Element<I> {
    Element::new(
        Kind::Slider {
            value,
            step: Factor16::from_bits(4096),
            intent,
        },
        Style::DEFAULT.focusable(true),
    )
}

/// A toggle, on or off.
#[must_use]
pub const fn toggle<I>(on: bool, intent: I) -> Element<I> {
    Element::new(Kind::Toggle { on, intent }, Style::DEFAULT.focusable(true))
}

/// Space with no content.
#[must_use]
pub const fn spacer<I>() -> Element<I> {
    Element::new(Kind::Spacer, Style::DEFAULT)
}

impl<I> Element<I> {
    /// The same element, with a slider's nudge this far.
    ///
    /// Nothing, on an element that is not a slider.
    #[must_use]
    pub const fn step(mut self, step: Factor16) -> Self {
        if let Kind::Slider { step: at, .. } = &mut self.kind {
            *at = step;
        }
        self
    }

    /// The same element, with a label's text breaking to fit or not.
    ///
    /// Nothing, on an element that is not a label.
    #[must_use]
    pub const fn wrap(mut self, wrap: bool) -> Self {
        if let Kind::Label { wrap: at, .. } = &mut self.kind {
            *at = wrap;
        }
        self
    }
}
