//! Laying a player's table over the one a game ships: the rules are on
//! [`Table::overlay`], where a reader of the public API can find them.

use alloc::collections::BTreeSet;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use crate::platform::bind::Bindings;
use crate::platform::table::Table;
use crate::sets::SetDescriptor;
use crate::source::{Axis, Button};

/// What laying a player's table over the shipped one came to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Overlaid {
    /// The table to play by.
    pub table: Table,
    /// Actions the player's table names that this build does not declare as
    /// the kind it binds them as, whose entries were left out.
    pub ignored: Vec<String>,
    /// Controls the game ships for an action the player left alone that were
    /// left off it, because the player drives something else in the same set
    /// with them.
    pub withheld: Vec<String>,
}

impl Table {
    /// Lays this table, a player's, over the table the game `shipped`.
    ///
    /// A player's table holds only what the player changed, and never the
    /// game's defaults, so it cannot go stale when the defaults do: a build
    /// that moves an action, adds one or drops one changes what a player who
    /// never touched that action plays with, and nothing on disk has to be
    /// brought up to date for that to be true.
    ///
    /// An action the player's table binds, as the kind this build declares it,
    /// takes exactly the controls the table gives it; so does an action named
    /// in [`unbound`](Table::unbound), which is how a player takes every control
    /// off one. Every other action keeps the controls the game ships for it,
    /// except any control the player has given to something else in the same
    /// set -- whether on its own, in a pair or as an axis -- because that one
    /// would fire both. Two sets never answer at once, so a control used in
    /// one does not keep it from an action in another.
    ///
    /// A name this build does not declare as that kind is left out and
    /// reported in [`Overlaid::ignored`] rather than refused. The table cannot
    /// say whether it is a word somebody mistyped or an action the game has
    /// since dropped, and refusing the second would stop a game from starting
    /// over its own rename. Leaving it on disk is what lets the build that
    /// still declares it go on reading it.
    ///
    /// Controls are compared by what they denote rather than how they are
    /// spelled, so `Mouse04` and `Mouse4` are one button. A control name this
    /// build cannot read is kept for [`to_bindings`](Table::to_bindings) to
    /// refuse.
    #[must_use]
    pub fn overlay(&self, sets: &[SetDescriptor], shipped: &Bindings) -> Overlaid {
        let declares = |names: fn(SetDescriptor) -> &'static [&'static str], action: &str| {
            sets.iter().any(|set| names(*set).contains(&action))
        };
        let digital = |action: &str| declares(SetDescriptor::digital_names, action);
        let analog = |action: &str| declares(SetDescriptor::analog_names, action);
        let mut ignored = BTreeSet::new();
        let mut keep = |declared: bool, action: &str| {
            if !declared {
                ignored.insert(action.to_string());
            }
            declared
        };
        let own = Self {
            buttons: self
                .buttons
                .iter()
                .filter(|e| keep(digital(&e.action), &e.action))
                .cloned()
                .collect(),
            axes: self
                .axes
                .iter()
                .filter(|e| keep(analog(&e.action), &e.action))
                .cloned()
                .collect(),
            pairs: self
                .pairs
                .iter()
                .filter(|e| keep(analog(&e.action), &e.action))
                .cloned()
                .collect(),
            unbound: Vec::new(),
        };
        for action in &self.unbound {
            keep(digital(action) || analog(action), action);
        }

        let unbound: BTreeSet<&str> = self.unbound.iter().map(String::as_str).collect();
        let chosen_digital: BTreeSet<&str> = own
            .buttons
            .iter()
            .map(|e| e.action.as_str())
            .chain(unbound.iter().copied())
            .collect();
        let chosen_analog: BTreeSet<&str> = own
            .axes
            .iter()
            .map(|e| e.action.as_str())
            .chain(own.pairs.iter().map(|e| e.action.as_str()))
            .chain(unbound.iter().copied())
            .collect();
        let set_of = |action: &str| {
            sets.iter().position(|set| {
                set.digital_names().contains(&action) || set.analog_names().contains(&action)
            })
        };
        let taken: BTreeSet<(Option<usize>, Control<'_>)> = uses(&own)
            .map(|(control, action)| (set_of(action), control))
            .collect();

        let mut table = Self::from_bindings(shipped, sets);
        let mut withheld = Vec::new();
        // Whether a shipped entry stays: the player chose nothing for its
        // action and uses none of its controls beside it, and any control they
        // do use is withheld.
        let mut stays = |chosen: &BTreeSet<&str>, action: &str, controls: &[Control<'_>]| {
            if chosen.contains(action) {
                return false;
            }
            let before = withheld.len();
            withheld.extend(
                controls
                    .iter()
                    .filter(|control| taken.contains(&(set_of(action), **control)))
                    .map(ToString::to_string),
            );
            withheld.len() == before
        };
        table
            .buttons
            .retain(|e| stays(&chosen_digital, &e.action, &[Control::button(&e.control)]));
        table
            .axes
            .retain(|e| stays(&chosen_analog, &e.action, &[Control::axis(&e.control)]));
        table.pairs.retain(|e| {
            let controls = [Control::button(&e.low), Control::button(&e.high)];
            stays(&chosen_analog, &e.action, &controls)
        });
        table.buttons.extend(own.buttons);
        table.axes.extend(own.axes);
        table.pairs.extend(own.pairs);
        Overlaid {
            table,
            ignored: ignored.into_iter().collect(),
            withheld,
        }
    }
}

/// A control compared by what it denotes rather than how a file spells it, so
/// that `Mouse04` and `Mouse4` are the one button they both name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Control<'a> {
    Button(Button),
    Axis(Axis),
    /// A name this build cannot read, which `to_bindings` refuses anyway.
    Unnamed(&'a str),
}

impl<'a> Control<'a> {
    fn button(name: &'a str) -> Self {
        Button::from_name(name).map_or(Self::Unnamed(name), Self::Button)
    }

    fn axis(name: &'a str) -> Self {
        Axis::from_name(name).map_or(Self::Unnamed(name), Self::Axis)
    }
}

/// The name the control is written down under, which is what
/// [`Overlaid::withheld`] reports.
impl fmt::Display for Control<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Button(button) => fmt::Display::fmt(button, f),
            Self::Axis(axis) => fmt::Display::fmt(axis, f),
            Self::Unnamed(name) => f.write_str(name),
        }
    }
}

/// Every control a table uses, beside the action it drives there; a pair's
/// two buttons count apart.
fn uses(table: &Table) -> impl Iterator<Item = (Control<'_>, &str)> {
    let buttons = table
        .buttons
        .iter()
        .map(|e| (Control::button(&e.control), e.action.as_str()));
    let axes = table
        .axes
        .iter()
        .map(|e| (Control::axis(&e.control), e.action.as_str()));
    let pairs = table.pairs.iter().flat_map(|e| {
        [
            (Control::button(&e.low), e.action.as_str()),
            (Control::button(&e.high), e.action.as_str()),
        ]
    });
    buttons.chain(axes).chain(pairs)
}

#[cfg(test)]
mod tests;
