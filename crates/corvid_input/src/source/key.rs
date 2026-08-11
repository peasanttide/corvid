//! Every key on the board, named by where it sits rather than what it says.

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
