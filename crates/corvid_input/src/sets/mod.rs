//! The declaration: what a set is, what it hands out, and the table it leaves
//! behind.

mod declare;
mod layout;

pub use layout::layout;

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
    /// total happened to be -- an empty run contains nothing, so nothing reads
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
    /// comparison, and it cannot be done the other way round -- `first + count`
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
/// taken apart in a `const` item -- which is where
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
    /// slice is as long as the range beside it -- a descriptor built by hand
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
