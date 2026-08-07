//! The device-neutral vocabulary a binding is written in.
//!
//! Nothing here names a windowing library, a scancode table or an operating
//! system. A key is named by where it sits on the board rather than by what is
//! printed on it, so a binding to [`Key::W`] is the same physical key on a
//! QWERTY board and on an AZERTY one — which is what a player who moves with
//! the three keys next to `A` actually means.
//!
//! Translating a platform's own event into one of these is the job of whoever
//! owns the event loop; `corvid_window` is the crate that does it for `winit`.
//!
//! Every type here writes itself down and reads itself back, because a binding
//! file names controls in text and a rebinding screen shows them in text, and
//! neither should keep a table of its own to do it.

use core::fmt;

/// Generates the key enum, its name table and its parser.
///
/// A key is written down in a binding file as the name it is declared under
/// here, so the name and the variant are one thing and are declared once.
macro_rules! keys {
    ($($name:ident => $text:literal),* $(,)?) => {
        /// One key, by its physical position on the board.
        ///
        /// The set is the one a game bound today can be played with, and it is
        /// deliberately not every key a keyboard has: a key that nothing can
        /// name is a key nothing can be bound to, and adding one later is a
        /// variant at the end of this list. Function keys, the numeric pad, the
        /// international keys and the media keys are not here.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[cfg_attr(
            feature = "serde",
            derive(::serde::Serialize, ::serde::Deserialize)
        )]
        #[non_exhaustive]
        pub enum Key {
            $(
                #[doc = ::core::concat!("The `", $text, "` key.")]
                $name,
            )*
        }

        impl Key {
            /// Every key this vocabulary names, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$name),*];

            /// The name this key is written down under.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$name => $text,)*
                }
            }

            /// The key a name denotes, or [`None`] if this vocabulary does not
            /// name it.
            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($text => Some(Self::$name),)*
                    _ => None,
                }
            }
        }
    };
}

keys! {
    A => "A", B => "B", C => "C", D => "D", E => "E", F => "F", G => "G",
    H => "H", I => "I", J => "J", K => "K", L => "L", M => "M", N => "N",
    O => "O", P => "P", Q => "Q", R => "R", S => "S", T => "T", U => "U",
    V => "V", W => "W", X => "X", Y => "Y", Z => "Z",
    Digit0 => "0", Digit1 => "1", Digit2 => "2", Digit3 => "3", Digit4 => "4",
    Digit5 => "5", Digit6 => "6", Digit7 => "7", Digit8 => "8", Digit9 => "9",
    Space => "Space", Enter => "Enter", Escape => "Escape", Tab => "Tab",
    Backspace => "Backspace",
    ArrowUp => "Up", ArrowDown => "Down", ArrowLeft => "Left",
    ArrowRight => "Right",
    LeftShift => "LeftShift", RightShift => "RightShift",
    LeftControl => "LeftControl", RightControl => "RightControl",
    LeftAlt => "LeftAlt", RightAlt => "RightAlt",
}

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
/// the bottom face button on all three — which is what a player who means "the
/// one under my thumb" actually means, and what each of those platforms tells a
/// developer to bind against.
///
/// The two triggers appear here *and* as an [`Axis`]: a trigger past a
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
        // that merely begins with it — the named ones were matched above.
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
/// them is not a variant — it needs a map from device to seat, which this
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
            // `Mouse` and nothing after it is not a button — the three above
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
/// A key is its position on the board — `W` is the key next to `A` on every
/// layout — and a mouse button is what [`MouseButton`] calls it.
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

/// One two-axis continuous control on a device.
///
/// **Two kinds, and which one a control is decides how a game must read it.**
/// The first two are *relative*: what arrives is how far something moved since
/// the last report, and a frame in which the platform said nothing is a frame in
/// which nothing moved. The three below are *absolute*: what arrives is where
/// the control is being held, which is still true on a frame the platform did
/// not mention it.
///
/// [`Reading`](crate::platform::Reading) is what a binding says about that, and
/// getting it wrong is silent either way: a stick bound as a displacement is a
/// control that works only while it is moving, and a mouse bound as a deflection
/// turns the camera by the square of the frame time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[non_exhaustive]
pub enum Axis {
    /// How far the mouse moved, in whatever unit the platform reports motion
    /// in — pixels on every desktop this workspace builds for.
    MouseMotion,
    /// How far the wheel turned, in whatever unit the platform reports a
    /// detent in.
    Scroll,
    /// How far the left stick is pushed, positive right and up.
    ///
    /// A *level* and not a motion, which is the whole difference between a
    /// stick and a mouse: it says where the stick is being held, so it answers
    /// on [`Input::analog`](crate::Input::analog) through a
    /// [`Reading::Deflection`](crate::platform::Reading) binding and the
    /// frame's `dt` multiplies it.
    LeftStick,
    /// The right stick, the same way.
    RightStick,
    /// How far the two triggers are pulled: `x` is the left one and `y` is the
    /// right.
    ///
    /// Two one-dimensional controls in one two-dimensional axis, because they
    /// are read together and reported together by every platform, and two
    /// variants would each waste a component. A game that wants one binds this
    /// and reads the component it cares about.
    ///
    /// Both run `0.0 ..= 1.0` rather than either side of zero: a trigger at
    /// rest is at rest, not centred.
    Triggers,
}

impl Axis {
    /// Every axis this vocabulary names, in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::MouseMotion,
        Self::Scroll,
        Self::LeftStick,
        Self::RightStick,
        Self::Triggers,
    ];

    /// The name this axis is written down under.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::MouseMotion => "MouseMotion",
            Self::Scroll => "Scroll",
            Self::LeftStick => "LeftStick",
            Self::RightStick => "RightStick",
            Self::Triggers => "Triggers",
        }
    }

    /// The axis a name denotes, or [`None`] if this vocabulary does not name
    /// it.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "MouseMotion" => Some(Self::MouseMotion),
            "Scroll" => Some(Self::Scroll),
            "LeftStick" => Some(Self::LeftStick),
            "RightStick" => Some(Self::RightStick),
            "Triggers" => Some(Self::Triggers),
            _ => None,
        }
    }
}

impl fmt::Display for Axis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
