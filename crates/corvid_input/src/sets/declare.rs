//! The macro a game writes its actions down in.
//!
//! Split from [the declaration](super) because a file stays under 400 lines,
//! and this is the seam that was already there: the parent is the data a
//! declaration leaves behind, and this is the spelling that produces it. The
//! documentation is most of the file, and deliberately so -- the numbering it
//! describes is what a saved binding file depends on, and the paragraph about
//! reordering is the one thing a reader has to take away.

/// Declares action sets, and with them every identifier a game names.
///
/// A game declares what it can be asked to do; hardware is bound to those
/// declarations somewhere else, and the game never sees a key code. This is
/// Steam Input's model, taken literally because it is the one that survives six
/// device kinds and a rebinding screen.
///
/// ```
/// /// The one place this game's actions are named.
/// pub mod action {
///     corvid_input::action_sets! {
///         pub set Menu {
///             digital NAVIGATE_UP, NAVIGATE_DOWN, ACTIVATE, BACK;
///         }
///         pub set Build {
///             digital PLACE, CANCEL;
///             analog LOOK, MOVE;
///             pose POINTER;
///         }
///     }
/// }
///
/// fn main() {
///     use corvid_input::{AnalogId, DigitalId, SetId};
///
///     // Sets are numbered in declaration order, and each kind of action is
///     // numbered from zero in its own space.
///     assert_eq!(action::Menu::ID, SetId(0));
///     assert_eq!(action::Build::ID, SetId(1));
///     assert_eq!(action::NAVIGATE_UP, DigitalId(0));
///     assert_eq!(action::PLACE, DigitalId(4));
///     assert_eq!(action::LOOK, AnalogId(0));
///
///     // And the table both of them came out of, which the binding layer and
///     // the rebinding screen read.
///     assert_eq!(action::SETS.len(), 2);
///     assert_eq!(action::SETS[1].name(), "Build");
///     assert!(action::SETS[1].digital().contains(action::PLACE.0));
///
///     // The names come with it, which is how a binding file names an action
///     // without naming the number that moves.
///     assert_eq!(corvid_input::digital_name(action::SETS, action::PLACE),
///                Some("PLACE"));
///     assert_eq!(corvid_input::analog_named(action::SETS, "LOOK"),
///                Some(action::LOOK));
/// }
/// ```
///
/// # What it generates
///
/// A `const SETS: &[SetDescriptor]` holding one [`SetDescriptor`](super::SetDescriptor) per set in
/// declaration order; a unit struct per set, named as the set was declared,
/// carrying that set's `ID` and `NAME` as associated constants; and one
/// constant per action, named as the action was declared, of the identifier
/// type its kind calls for.
///
/// Action constants land in the module the macro was invoked from rather than
/// inside their set's struct, so a game writes `action::LOOK`. Two actions of
/// any kind sharing a name is therefore a duplicate-definition error, which is
/// the same rule Steam Input's manifest works under. `SETS` is generated with
/// the same name every time, so one invocation per module.
///
/// # Declaration order is a wire format
///
/// The identifiers come from declaration order and from nothing else. A binding
/// file records that this player's `X` button is `DigitalId(4)`; move the set
/// that owns `DigitalId(4)` and the file now points at somebody else's action.
/// **Reordering a declaration, or inserting an action anywhere but at the end
/// of its set, re-points the run of identifiers from the edit onwards.**
///
/// A run, and not every binding saved. Identifiers ahead of the edit keep their
/// numbers, and so does every identifier of a kind the edit did not disturb:
/// swap the two sets in the example above and `LOOK` is still `AnalogId(0)`,
/// because `Menu` declares no analog actions and `Build`'s analog run therefore
/// never moved, while every digital action in both sets did. `tests/sets.rs`
/// makes that swap on a longer declaration and asserts both halves of it, and
/// the crate's README makes it on this one. A run is the more dangerous of the
/// two possible shapes, because a migration that checks one binding and finds it
/// unmoved has learnt nothing about the rest. Nothing here can detect the break,
/// because the file was written by an older build that is not present to be
/// compared against -- which is why `tests/golden.rs` freezes the numbering as
/// literals, so the change is at least a red test at home before it is a wrong
/// button in front of a player. Adding a set at the end, or an action at the end
/// of the last set of its kind, moves nothing.
///
/// The expansion recurses once per action, so a set with a great many actions
/// of one kind may need `#![recursion_limit]` raised.
#[macro_export]
macro_rules! action_sets {
    (
        $(
            $vis:vis set $set:ident {
                $(digital $($digital:ident),* $(,)? ;)?
                $(analog $($analog:ident),* $(,)? ;)?
                $(pose $($pose:ident),* $(,)? ;)?
            }
        )*
    ) => {
        /// Every action set declared here, in declaration order.
        ///
        /// Index `n` is the set numbered `n`. This is the table the binding
        /// layer and the rebinding screen read, and the table
        /// [`Input`](::corvid_input::Input) reads to decide whether a query is
        /// asking about the active set.
        pub const SETS: &[$crate::SetDescriptor] = &$crate::layout(&[
            $(
                $crate::SetNames {
                    name: ::core::stringify!($set),
                    // The name a binding file writes down is the identifier the
                    // action was declared under, spelled exactly as it appears
                    // here. It is the one thing about an action that does not
                    // move when a declaration is reordered.
                    digital: &[$($(::core::stringify!($digital)),*)?],
                    analog: &[$($(::core::stringify!($analog)),*)?],
                    pose: &[$($(::core::stringify!($pose)),*)?],
                },
            )*
        ]);

        $crate::action_sets!(@sets 0usize; $(
            $vis set $set {
                digital $($($digital),*)?;
                analog $($($analog),*)?;
                pose $($($pose),*)?;
            }
        )*);
    };

    (@sets $index:expr; ) => {};
    (@sets $index:expr;
        $vis:vis set $set:ident {
            digital $($digital:ident),* ;
            analog $($analog:ident),* ;
            pose $($pose:ident),* ;
        }
        $($rest:tt)*
    ) => {
        #[doc = ::core::concat!(
            "The `", ::core::stringify!($set), "` action set.\n\nA marker for \
             the set's number and name; the actions themselves are constants in \
             this module."
        )]
        #[derive(::core::clone::Clone, ::core::marker::Copy, ::core::fmt::Debug)]
        #[derive(::core::cmp::PartialEq, ::core::cmp::Eq, ::core::hash::Hash)]
        $vis struct $set;

        impl $set {
            #[doc = ::core::concat!(
                "The number `", ::core::stringify!($set), "` was assigned, from \
                 its position in the declaration."
            )]
            $vis const ID: $crate::SetId = SETS[$index].id();

            #[doc = ::core::concat!(
                "The name `", ::core::stringify!($set), "` was declared under, \
                 as text, for a rebinding screen to show."
            )]
            $vis const NAME: &'static str = SETS[$index].name();
        }

        $crate::action_sets!(@digital $vis $set, SETS[$index].digital().first(), $($digital),*);
        $crate::action_sets!(@analog $vis $set, SETS[$index].analog().first(), $($analog),*);
        $crate::action_sets!(@pose $vis $set, SETS[$index].pose().first(), $($pose),*);

        $crate::action_sets!(@sets $index + 1usize; $($rest)*);
    };

    (@digital $vis:vis $set:ident, $base:expr, ) => {};
    (@digital $vis:vis $set:ident, $base:expr, $name:ident $(, $rest:ident)* $(,)?) => {
        #[doc = ::core::concat!(
            "The `", ::core::stringify!($name), "` digital action of the `",
            ::core::stringify!($set), "` set."
        )]
        $vis const $name: $crate::DigitalId = $crate::DigitalId($base);
        $crate::action_sets!(@digital $vis $set, $base + 1, $($rest),*);
    };

    (@analog $vis:vis $set:ident, $base:expr, ) => {};
    (@analog $vis:vis $set:ident, $base:expr, $name:ident $(, $rest:ident)* $(,)?) => {
        #[doc = ::core::concat!(
            "The `", ::core::stringify!($name), "` analog action of the `",
            ::core::stringify!($set), "` set."
        )]
        $vis const $name: $crate::AnalogId = $crate::AnalogId($base);
        $crate::action_sets!(@analog $vis $set, $base + 1, $($rest),*);
    };

    (@pose $vis:vis $set:ident, $base:expr, ) => {};
    (@pose $vis:vis $set:ident, $base:expr, $name:ident $(, $rest:ident)* $(,)?) => {
        #[doc = ::core::concat!(
            "The `", ::core::stringify!($name), "` pose of the `",
            ::core::stringify!($set), "` set."
        )]
        $vis const $name: $crate::PoseId = $crate::PoseId($base);
        $crate::action_sets!(@pose $vis $set, $base + 1, $($rest),*);
    };
}
