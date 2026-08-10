//! The three kinds of control that are on or off, and the one that names any
//! of them.

use core::fmt;

use super::Key;

/// One mouse button.
///
/// [`Other`](Self::Other) carries the platform's own number for a button past
/// the three every mouse has, because a side button is worth binding and
/// inventing a name for each is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[non_exhaustive]
pub enum MouseButton {
    /// The primary button, whichever hand the player has it set up for.
    Left,
    /// The secondary button.
    Right,
    /// The wheel pressed as a button.
    Middle,
    /// Any further button, by the number the platform gave it.
    Other(u16),
}

/// One button on a gamepad, by **where it sits** rather than by what is
/// printed on it.
///
/// The same rule [`Key`] follows, and for a stronger reason: the face buttons
/// are lettered `A B X Y` on an Xbox pad, `B A Y X` on a Nintendo one, and
/// drawn as shapes on a `PlayStation` one. A binding to [`South`](Self::South) is
/// the bottom face button on all three -- which is what a player who means "the
/// one under my thumb" actually means, and what each of those platforms tells a
/// developer to bind against.
///
/// The two triggers appear here *and* as an [`Axis`](super::Axis): a trigger past a
/// threshold is a button, and how far it is pulled is an axis, and a game may
/// want either. Which one a control drives is the binding table's to say.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[non_exhaustive]
pub enum PadButton {
    /// The bottom face button: `A` on an Xbox pad, cross on a `PlayStation` one.
    South,
    /// The right face button: `B`, circle.
    East,
    /// The left face button: `X`, square.
    West,
    /// The top face button: `Y`, triangle.
    North,
    /// The left shoulder button.
    LeftBumper,
    /// The right shoulder button.
    RightBumper,
    /// The left trigger, past the point where it counts as pressed.
    LeftTrigger,
    /// The right trigger.
    RightTrigger,
    /// The left-hand small button: `Back`, `Select`, `Share`.
    Select,
    /// The right-hand small button: `Start`, `Options`.
    Start,
    /// The middle button, which many platforms reserve.
    Guide,
    /// The left stick pressed in.
    LeftStick,
    /// The right stick pressed in.
    RightStick,
    /// Up on the directional pad.
    PadUp,
    /// Down on it.
    PadDown,
    /// Left on it.
    PadLeft,
    /// Right on it.
    PadRight,
    /// Any further button, by the number the platform gave it.
    Other(u16),
}

impl PadButton {
    /// Every named button, in declaration order.
    ///
    /// [`Other`](Self::Other) is not here, because it is a family rather than a
    /// button.
    pub const ALL: &'static [Self] = &[
        Self::South,
        Self::East,
        Self::West,
        Self::North,
        Self::LeftBumper,
        Self::RightBumper,
        Self::LeftTrigger,
        Self::RightTrigger,
        Self::Select,
        Self::Start,
        Self::Guide,
        Self::LeftStick,
        Self::RightStick,
        Self::PadUp,
        Self::PadDown,
        Self::PadLeft,
        Self::PadRight,
    ];

    /// The name this button is written down under.
    ///
    /// [`None`] for [`Other`](Self::Other), which carries a number and is
    /// formatted rather than named.
    #[must_use]
    pub const fn name(self) -> Option<&'static str> {
        Some(match self {
            Self::South => "PadSouth",
            Self::East => "PadEast",
            Self::West => "PadWest",
            Self::North => "PadNorth",
            Self::LeftBumper => "PadLeftBumper",
            Self::RightBumper => "PadRightBumper",
            Self::LeftTrigger => "PadLeftTrigger",
            Self::RightTrigger => "PadRightTrigger",
            Self::Select => "PadSelect",
            Self::Start => "PadStart",
            Self::Guide => "PadGuide",
            Self::LeftStick => "PadLeftStick",
            Self::RightStick => "PadRightStick",
            Self::PadUp => "PadUp",
            Self::PadDown => "PadDown",
            Self::PadLeft => "PadLeft",
            Self::PadRight => "PadRight",
            Self::Other(_) => return None,
        })
    }

    /// The button a name denotes, or [`None`] if this vocabulary does not name
    /// it.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        if let Some(found) = Self::ALL.iter().find(|button| button.name() == Some(name)) {
            return Some(*found);
        }
        // `Pad` and nothing after it is not a button, and neither is a name
        // that merely begins with it -- the named ones were matched above.
        name.strip_prefix("Pad")
            .filter(|rest| !rest.is_empty())
            .and_then(|rest| rest.parse().ok())
            .map(Self::Other)
    }
}

impl fmt::Display for PadButton {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => f.write_str(name),
            None => match self {
                Self::Other(number) => write!(f, "Pad{number}"),
                // Unreachable while `name` answers for every named variant, and
                // the honest fallback for one added without a name.
                _ => f.write_str("Pad"),
            },
        }
    }
}

/// One on-or-off control on a device.
///
/// Three device kinds, and adding a fourth is a variant at the end rather than
/// a change to anything already written down.
///
/// **There is no pad number here, and that is a decision.** Every pad the
/// platform reports folds into one set of controls, so two pads plugged into
/// one machine are two hands on one seat rather than two players. Splitting
/// them is not a variant -- it needs a map from device to seat, which this
/// workspace does not have and which a local-multiplayer game would design
/// rather than inherit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[non_exhaustive]
pub enum Button {
    /// A key on the board.
    Key(Key),
    /// A button on the mouse.
    Mouse(MouseButton),
    /// A button on a gamepad.
    Pad(PadButton),
}

impl Button {
    /// A key, as a button.
    #[must_use]
    #[inline]
    pub const fn key(key: Key) -> Self {
        Self::Key(key)
    }

    /// A mouse button, as a button.
    #[must_use]
    #[inline]
    pub const fn mouse(button: MouseButton) -> Self {
        Self::Mouse(button)
    }

    /// A gamepad button, as a button.
    #[must_use]
    #[inline]
    pub const fn pad(button: PadButton) -> Self {
        Self::Pad(button)
    }
}

/// How a mouse button is written down, and read back.
///
/// The three every mouse has are named; anything further is `Mouse` and the
/// platform's own number, which is the only honest thing to call a button whose
/// label is on somebody's desk and not in this enum.
impl fmt::Display for MouseButton {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Left => f.write_str("MouseLeft"),
            Self::Right => f.write_str("MouseRight"),
            Self::Middle => f.write_str("MouseMiddle"),
            Self::Other(number) => write!(f, "Mouse{number}"),
        }
    }
}

impl MouseButton {
    /// The button a name denotes, or [`None`] if this vocabulary does not name
    /// it.
    ///
    /// Exactly what [`Display`](fmt::Display) writes, so a binding file
    /// round-trips.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "MouseLeft" => Some(Self::Left),
            "MouseRight" => Some(Self::Right),
            "MouseMiddle" => Some(Self::Middle),
            // `Mouse` and nothing after it is not a button -- the three above
            // are the named ones and a further one carries its number.
            other => other
                .strip_prefix("Mouse")
                .filter(|rest| !rest.is_empty())
                .and_then(|rest| rest.parse().ok())
                .map(Self::Other),
        }
    }
}

/// How a control is written down in a binding file, and shown on a rebinding
/// screen.
///
/// A key is its position on the board -- `W` is the key next to `A` on every
/// layout -- and a mouse button is what [`MouseButton`] calls it.
impl fmt::Display for Button {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key(key) => f.write_str(key.name()),
            Self::Mouse(button) => fmt::Display::fmt(button, f),
            Self::Pad(button) => fmt::Display::fmt(button, f),
        }
    }
}

impl Button {
    /// The control a name denotes, or [`None`] if this vocabulary does not name
    /// it.
    ///
    /// Keys first, because there are forty of them and three mouse buttons, and
    /// because no key is spelled like a mouse button.
    ///
    /// ```
    /// use corvid_input::{Button, Key};
    ///
    /// assert_eq!(Button::from_name("W"), Some(Button::key(Key::W)));
    /// assert_eq!(Button::from_name("MouseLeft").map(|b| b.to_string()),
    ///            Some("MouseLeft".to_owned()));
    /// assert_eq!(Button::from_name("Foot"), None);
    /// ```
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Key::from_name(name)
            .map(Self::Key)
            .or_else(|| MouseButton::from_name(name).map(Self::Mouse))
            .or_else(|| PadButton::from_name(name).map(Self::Pad))
    }
}
