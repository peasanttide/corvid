//! The declaration: what a set is, what it hands out, and the table it leaves
//! behind.

use crate::id::SetId;

/// A run of consecutive identifiers, which is what one set owns of one kind.
///
/// [`action_sets!`](crate::action_sets) numbers each kind densely from zero
/// across the whole declaration, so every set's actions of a kind land next to
/// each other and asking whether an identifier belongs to a set is two
/// comparisons rather than a search.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct IdRange {
    first: u16,
    count: u16,
}

impl IdRange {
    /// A set that declared none of this kind.
    pub const EMPTY: Self = Self { first: 0, count: 0 };

    /// A run of `count` identifiers starting at `first`.
    #[must_use]
    #[inline]
    pub const fn new(first: u16, count: u16) -> Self {
        Self { first, count }
    }

    /// The lowest identifier in the run.
    ///
    /// Meaningless when the run is empty, where it is whatever the running
    /// total happened to be — an empty run contains nothing, so nothing reads
    /// this without having checked.
    #[must_use]
    #[inline]
    pub const fn first(self) -> u16 {
        self.first
    }

    /// How many identifiers are in the run.
    #[must_use]
    #[inline]
    pub const fn count(self) -> u16 {
        self.count
    }

    /// Whether the run is empty.
    #[must_use]
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.count == 0
    }

    /// Whether `id` is one of the identifiers in the run.
    ///
    /// The subtraction cannot go below zero because `&&` stops at the first
    /// comparison, and it cannot be done the other way round — `first + count`
    /// would be the thing that overflowed instead, on a run that ends at
    /// `u16::MAX`.
    #[must_use]
    #[inline]
    pub const fn contains(self, id: u16) -> bool {
        id >= self.first && id - self.first < self.count
    }
}

/// What one set declared, before the identifiers were handed out.
///
/// This is [`action_sets!`](crate::action_sets)'s input to [`layout`] and is
/// public for that reason alone. A table built by hand goes through the same
/// door.
///
/// The actions arrive as their **names**, and the count of a kind is the length
/// of its slice. Names rather than counts because a binding file has to name an
/// action in text: an identifier is handed out from declaration order, which
/// this crate's documentation calls a wire format, so a file that recorded the
/// number would re-point itself the day somebody inserted an action. The name a
/// programmer wrote is the one thing about an action that does not move.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SetNames {
    /// The set's name, which is the identifier it was declared under.
    pub name: &'static str,
    /// Its digital actions, named, in declaration order.
    pub digital: &'static [&'static str],
    /// Its analog actions.
    pub analog: &'static [&'static str],
    /// Its poses.
    pub pose: &'static [&'static str],
}

/// One action set as a binding layer and a rebinding screen read it: a set's
/// number, the name it was declared under, and which identifiers of each kind
/// belong to it.
///
/// [`Input`](crate::Input) reads it too, to decide whether a query is asking
/// about the active set.
///
/// The fields are private and the accessors are `const`, so a descriptor can be
/// taken apart in a `const` item — which is where
/// [`action_sets!`](crate::action_sets) takes it apart, to give each action its
/// number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SetDescriptor {
    id: SetId,
    declared: SetNames,
    digital: IdRange,
    analog: IdRange,
    pose: IdRange,
}

impl SetDescriptor {
    /// A nameless set that declared nothing, which is what [`layout`] fills an
    /// array with before it writes the real ones over the top.
    pub const EMPTY: Self = Self {
        id: SetId(0),
        declared: SetNames {
            name: "",
            digital: &[],
            analog: &[],
            pose: &[],
        },
        digital: IdRange::EMPTY,
        analog: IdRange::EMPTY,
        pose: IdRange::EMPTY,
    };

    /// Assembles a descriptor from a declaration that has already been
    /// numbered.
    ///
    /// `declared` carries the names in declaration order, so its `digital[n]`
    /// is the action numbered `digital.first() + n`. Nothing here checks that a
    /// slice is as long as the range beside it — a descriptor built by hand
    /// with fewer names simply has actions that no file can name, which
    /// [`digital_name`] reports as [`None`].
    #[must_use]
    #[inline]
    pub const fn new(
        id: SetId,
        declared: SetNames,
        digital: IdRange,
        analog: IdRange,
        pose: IdRange,
    ) -> Self {
        Self {
            id,
            declared,
            digital,
            analog,
            pose,
        }
    }

    /// The set's number.
    #[must_use]
    #[inline]
    pub const fn id(self) -> SetId {
        self.id
    }

    /// The identifier the set was declared under, as text.
    #[must_use]
    #[inline]
    pub const fn name(self) -> &'static str {
        self.declared.name
    }

    /// The digital actions the set owns.
    #[must_use]
    #[inline]
    pub const fn digital(self) -> IdRange {
        self.digital
    }

    /// The analog actions the set owns.
    #[must_use]
    #[inline]
    pub const fn analog(self) -> IdRange {
        self.analog
    }

    /// The poses the set owns.
    #[must_use]
    #[inline]
    pub const fn pose(self) -> IdRange {
        self.pose
    }

    /// This set's digital actions by name, in declaration order.
    #[must_use]
    #[inline]
    pub const fn digital_names(self) -> &'static [&'static str] {
        self.declared.digital
    }

    /// Its analog actions.
    #[must_use]
    #[inline]
    pub const fn analog_names(self) -> &'static [&'static str] {
        self.declared.analog
    }

    /// Its poses.
    #[must_use]
    #[inline]
    pub const fn pose_names(self) -> &'static [&'static str] {
        self.declared.pose
    }
}

/// Declares the four lookups over a declaration, which differ only in which
/// kind they read.
///
/// A macro rather than four hand-written pairs, because the pairs are the same
/// eight lines with one accessor changed, and a copy that drifted would be a
/// binding file that reads one kind of action correctly and another kind
/// silently wrong.
macro_rules! lookup {
    (
        $named:ident, $name_of:ident, $id:ident, $range:ident, $names:ident,
        $kind:literal
    ) => {
        #[doc = ::core::concat!(
                            "The ", $kind, " action `name` denotes, or [`None`] if no set in \
             `sets` declared one under that name.\n\nA linear walk: a \
             declaration is a handful of sets and this is read when a file is \
             loaded or a rebinding screen is drawn, never per frame."
                        )]
        #[must_use]
        pub fn $named(sets: &[SetDescriptor], name: &str) -> Option<crate::id::$id> {
            for set in sets {
                for (offset, declared) in set.$names().iter().enumerate() {
                    if *declared == name {
                        let offset = u16::try_from(offset).ok()?;
                        return Some(crate::id::$id(set.$range().first().checked_add(offset)?));
                    }
                }
            }
            None
        }

        #[doc = ::core::concat!(
                            "What `id` was declared as, or [`None`] if no set in `sets` owns \
             it.\n\nThe inverse of the lookup above, and what writes a binding \
             file down."
                        )]
        #[must_use]
        pub fn $name_of(sets: &[SetDescriptor], id: crate::id::$id) -> Option<&'static str> {
            for set in sets {
                let range = set.$range();
                if range.contains(id.0) {
                    return set.$names().get(usize::from(id.0 - range.first())).copied();
                }
            }
            None
        }
    };
}

lookup!(
    digital_named,
    digital_name,
    DigitalId,
    digital,
    digital_names,
    "digital"
);
lookup!(
    analog_named,
    analog_name,
    AnalogId,
    analog,
    analog_names,
    "analog"
);
lookup!(pose_named, pose_name, PoseId, pose, pose_names, "pose");

/// Turns declaration order into identifiers.
///
/// Sets are numbered from zero in the order they arrive, and each kind of
/// action is numbered from zero in its own space, so the ranges of a kind
/// partition that space in declaration order with no gaps. That is the whole of
/// the numbering rule, and it is a wire format: a binding file saved by
/// yesterday's build names these numbers, so moving a declaration re-points
/// every binding at or after it.
///
/// ```
/// use corvid_input::{IdRange, SetNames, layout};
///
/// const TABLE: [corvid_input::SetDescriptor; 2] = layout(&[
///     SetNames {
///         name: "Menu",
///         digital: &["UP", "DOWN", "ACTIVATE", "BACK"],
///         analog: &[],
///         pose: &[],
///     },
///     SetNames {
///         name: "Build",
///         digital: &["PLACE", "CANCEL"],
///         analog: &["LOOK", "MOVE"],
///         pose: &["POINTER"],
///     },
/// ]);
///
/// // The second set's digital actions continue where the first's stopped, and
/// // its analog actions start over, because the two kinds are numbered apart.
/// assert_eq!(TABLE[1].digital(), IdRange::new(4, 2));
/// assert_eq!(TABLE[1].analog(), IdRange::new(0, 2));
///
/// // And the names came along, which is what a binding file writes down.
/// assert_eq!(corvid_input::digital_named(&TABLE, "PLACE"),
///            Some(corvid_input::DigitalId(4)));
/// ```
///
/// # Panics
///
/// When a declaration has more identifiers of one kind than a `u16` can
/// number. From a `const` item — which is the only place
/// [`action_sets!`](crate::action_sets) calls this — that panic is a compile
/// error, and it is one in every profile:
///
/// ```rust,compile_fail
/// use corvid_input::{SetDescriptor, SetNames, layout};
///
/// const TOO_MANY: [SetDescriptor; 2] = layout(&[
///     SetNames { name: "First", digital: &["a"; 40_000], analog: &[], pose: &[] },
///     SetNames { name: "Second", digital: &["b"; 40_000], analog: &[], pose: &[] },
/// ]);
/// assert_eq!(TOO_MANY.len(), 2);
/// ```
///
/// It is a panic rather than a wrapping `+` because the two are not the same
/// thing here. Const evaluation rejects a `+` that overflows only when the
/// profile has overflow checks on, so plain arithmetic would have made this a
/// compile error in a `dev` build and a table of overlapping identifiers in a
/// `release` one — the same declaration numbered two different ways by two
/// builds of the same game, which is the one outcome a wire format cannot have.
#[must_use]
pub const fn layout<const N: usize>(counts: &[SetNames; N]) -> [SetDescriptor; N] {
    let mut table = [SetDescriptor::EMPTY; N];
    let mut index = 0;
    let mut id: u16 = 0;
    let mut digital: u16 = 0;
    let mut analog: u16 = 0;
    let mut pose: u16 = 0;

    while index < N {
        let set = &counts[index];
        // A kind's count is how many names it declared. `usize` to `u16` is
        // the one narrowing here, and it refuses rather than wrapping for the
        // reason `advance` does.
        let digital_count = count(set.digital.len());
        let analog_count = count(set.analog.len());
        let pose_count = count(set.pose.len());
        table[index] = SetDescriptor::new(
            SetId(id),
            *set,
            IdRange::new(digital, digital_count),
            IdRange::new(analog, analog_count),
            IdRange::new(pose, pose_count),
        );
        digital = advance(digital, digital_count);
        analog = advance(analog, analog_count);
        pose = advance(pose, pose_count);
        index += 1;
        // The last set has no successor to number, and asking for one would be
        // the only step in this loop that could refuse a declaration that was
        // otherwise fine.
        if index < N {
            id = advance(id, 1);
        }
    }

    table
}

/// How many identifiers a slice of names is, as a `u16`.
///
/// # Panics
///
/// When a set declares more than 65 535 actions of one kind. A compile error
/// from a `const` item, for the reason [`advance`] gives.
#[allow(
    clippy::panic,
    reason = "const evaluation turns this into a compile error, which is the whole point: see `advance`"
)]
#[allow(
    clippy::cast_possible_truncation,
    reason = "the range pattern is what makes the cast exact"
)]
const fn count(names: usize) -> u16 {
    match names {
        0..=0xFFFF => names as u16,
        _ => panic!("an action set declares more actions of one kind than a u16 can number"),
    }
}

/// Moves a running total on, refusing to wrap.
///
/// # Panics
///
/// When the total does not fit in a `u16`. See [`layout`], which is the only
/// caller and whose documentation explains why this is a panic.
#[allow(
    clippy::panic,
    reason = "const evaluation turns this into a compile error, which is the whole point: it is the only way to refuse an overflowing declaration in a profile that has turned overflow checks off, and the workspace ships one of those"
)]
const fn advance(total: u16, by: u16) -> u16 {
    match total.checked_add(by) {
        Some(next) => next,
        None => panic!(
            "an action set declaration has more identifiers of one kind than a u16 can number"
        ),
    }
}

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
/// A `const SETS: &[SetDescriptor]` holding one [`SetDescriptor`] per set in
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
/// compared against — which is why `tests/golden.rs` freezes the numbering as
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
