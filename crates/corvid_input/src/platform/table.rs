//! A binding table written down: controls and actions by name, never by
//! number.
//!
//! [`Bindings`] is what a device layer reads and it holds identifiers, because
//! an identifier is what a snapshot is indexed by. A **file** cannot hold
//! those. Identifiers come from declaration order, which this crate's
//! documentation calls a wire format in as many words: insert an action
//! anywhere but at the end of its set and every identifier from there on moves,
//! so a file that recorded `4` would point at somebody else's action the next
//! time the game was built, silently and with nothing anywhere to compare
//! against.
//!
//! The name a programmer declared an action under does not move. So that is
//! what goes in the file, and this is the type that stands between the two.
//!
//! This crate supplies the parts and not the file: what is here is
//! `Serialize` and `Deserialize`, and which format those are driven by — JSON,
//! in `corvid_app` — is somebody else's decision, exactly as the `serde`
//! feature already says of the identifiers themselves.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use core::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::platform::bind::{Bindings, Component, Reading};
use crate::sets::{SetDescriptor, analog_name, analog_named, digital_name, digital_named};
use crate::source::{Axis, Button};

/// One control driving one digital action, by name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ButtonEntry {
    /// What the player presses, as [`Button`] writes itself down: `W`,
    /// `Space`, `MouseLeft`.
    pub control: String,
    /// What it does, as the game declared it: `FORWARD`.
    pub action: String,
}

/// One control driving one analog action, by name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AxisEntry {
    /// Which control: `MouseMotion` or `Scroll`.
    pub control: String,
    /// What it drives.
    pub action: String,
    /// How many of the device's own units make a full sweep. Smaller is more
    /// sensitive; zero is refused rather than clamped, because a player who
    /// typed it meant something and no reading of it is right.
    pub span: u32,
    /// Whether the action answers on `Input::analog` or `Input::delta`, and
    /// therefore whether what a game reads is a rate or a quantity.
    pub reading: Reading,
}

/// Two buttons standing in for one axis, by name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PairEntry {
    /// The control that pushes the axis negative.
    pub low: String,
    /// The one that pushes it positive.
    pub high: String,
    /// What they drive.
    pub action: String,
    /// Which of that action's two components: `X` or `Y`.
    pub component: Component,
}

/// A binding table as a file holds it.
///
/// Lists rather than maps keyed by action, because [`Bindings`] promises that
/// several controls may drive one action and one control several — the first is
/// how a game is playable with either hand — and a map can hold neither.
/// Order is kept on the way in and on the way out, because the table's own
/// accessors promise "in the order it was added".
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Table {
    /// Every button binding.
    #[serde(default)]
    pub buttons: Vec<ButtonEntry>,
    /// Every axis binding.
    #[serde(default)]
    pub axes: Vec<AxisEntry>,
    /// Every pair of buttons standing in for an axis.
    ///
    /// No `reading` here, and no `span`: a pair is always a deflection and a
    /// full press is always a full deflection, so there is nothing for a file
    /// to get wrong about either.
    #[serde(default)]
    pub pairs: Vec<PairEntry>,
}

/// Something in a table did not name anything this build has.
///
/// It carries what was written rather than a position in a file, because the
/// file is somebody else's format and a byte offset into it is not this
/// module's to know. What a player needs is the word they got wrong, and that
/// is here.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Unknown {
    /// No set in the declaration declares a digital action under this name.
    DigitalAction(String),
    /// Nor an analog one.
    AnalogAction(String),
    /// The control vocabulary does not name this control.
    Control(String),
    /// Nor this axis.
    Axis(String),
    /// A span of zero, which is a divisor of zero.
    Span(String),
}

impl fmt::Display for Unknown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DigitalAction(name) => {
                write!(f, "this game declares no action called {name}")
            }
            Self::AnalogAction(name) => {
                write!(f, "this game declares no analog action called {name}")
            }
            Self::Control(name) => write!(f, "{name} is not a control this build can name"),
            Self::Axis(name) => write!(f, "{name} is not an axis this build can name"),
            Self::Span(action) => write!(f, "the span of {action} is zero, which cannot divide"),
        }
    }
}

impl core::error::Error for Unknown {}

impl Table {
    /// Writes a table down against a declaration.
    ///
    /// A binding whose action the declaration does not name is **left out**
    /// rather than refused. This direction is used to write a file for a player
    /// to edit, and an identifier with no name is one a game bound by hand
    /// outside its own declaration — there is nothing to call it in a file, and
    /// stopping a run from starting over a binding nobody can name would be the
    /// wrong end to fail at. The other direction refuses, because there a name
    /// somebody typed did not match.
    #[must_use]
    pub fn from_bindings(bindings: &Bindings, sets: &[SetDescriptor]) -> Self {
        let buttons = bindings
            .buttons()
            .iter()
            .filter_map(|&(control, action)| {
                digital_name(sets, action).map(|action| ButtonEntry {
                    control: control.to_string(),
                    action: action.to_string(),
                })
            })
            .collect();
        let axes = bindings
            .axes()
            .iter()
            .filter_map(|binding| {
                analog_name(sets, binding.action).map(|action| AxisEntry {
                    control: binding.axis.name().to_string(),
                    action: action.to_string(),
                    span: binding.span.get(),
                    reading: binding.reading,
                })
            })
            .collect();
        let pairs = bindings
            .pairs()
            .iter()
            .filter_map(|binding| {
                analog_name(sets, binding.action).map(|action| PairEntry {
                    low: binding.low.to_string(),
                    high: binding.high.to_string(),
                    action: action.to_string(),
                    component: binding.component,
                })
            })
            .collect();
        Self {
            buttons,
            axes,
            pairs,
        }
    }

    /// Reads it back against the same declaration.
    ///
    /// # Errors
    ///
    /// [`Unknown`] for the first entry naming an action, a control or an axis
    /// this build does not have, or a span of zero. The first rather than all
    /// of them: a file with one typo in it is the common case, and a player
    /// fixes one line and runs the game again.
    pub fn to_bindings(&self, sets: &[SetDescriptor]) -> Result<Bindings, Unknown> {
        let mut table = Bindings::new();
        for entry in &self.buttons {
            let control = Button::from_name(&entry.control)
                .ok_or_else(|| Unknown::Control(entry.control.clone()))?;
            let action = digital_named(sets, &entry.action)
                .ok_or_else(|| Unknown::DigitalAction(entry.action.clone()))?;
            table = table.button(control, action);
        }
        for entry in &self.axes {
            let control = Axis::from_name(&entry.control)
                .ok_or_else(|| Unknown::Axis(entry.control.clone()))?;
            let action = analog_named(sets, &entry.action)
                .ok_or_else(|| Unknown::AnalogAction(entry.action.clone()))?;
            let span =
                NonZeroU32::new(entry.span).ok_or_else(|| Unknown::Span(entry.action.clone()))?;
            table = table.axis(control, action, span, entry.reading);
        }
        for entry in &self.pairs {
            let low =
                Button::from_name(&entry.low).ok_or_else(|| Unknown::Control(entry.low.clone()))?;
            let high = Button::from_name(&entry.high)
                .ok_or_else(|| Unknown::Control(entry.high.clone()))?;
            let action = analog_named(sets, &entry.action)
                .ok_or_else(|| Unknown::AnalogAction(entry.action.clone()))?;
            table = table.pair(low, high, action, entry.component);
        }
        Ok(table)
    }
}
