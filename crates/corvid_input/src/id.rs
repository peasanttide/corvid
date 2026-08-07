//! The numbered identifiers, which are all the same type with different names.
//!
//! Each is a newtype over a `u16` with the field public, because an identifier
//! is a number and hiding it behind an accessor would cost every caller a line
//! and buy nothing. What the newtype buys is that a
//! [`DigitalId`](crate::DigitalId) cannot be passed where an
//! [`AnalogId`](crate::AnalogId) was meant — the two are numbered in separate
//! spaces, so `0` is a different action in each, and a mix-up would compile and
//! then read the wrong stick.
//!
//! The numbers themselves are handed out by
//! [`action_sets!`](crate::action_sets) from declaration order. That makes them
//! a wire format: a binding file saved yesterday names them, and nothing in the
//! type system notices when a declaration moves.

/// Declares one of the identifiers.
///
/// `serde` is `transparent`, so an identifier is written down as the bare
/// number it is. A binding file is somebody else's format and this crate only
/// supplies the parts, so the parts should not each arrive wrapped in a
/// single-field object.
macro_rules! id_type {
    (
        $(#[$meta:meta])*
        $name:ident, $field_doc:literal
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[cfg_attr(
            feature = "serde",
            derive(::serde::Serialize, ::serde::Deserialize),
            serde(transparent)
        )]
        pub struct $name(
            #[doc = $field_doc]
            pub u16,
        );

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.0, f)
            }
        }
    };
}

id_type! {
    /// One on-or-off action: a button, a key, a trigger past its threshold.
    ///
    /// Numbered from zero across the whole declaration, in declaration order,
    /// so the first set's digital actions come first and the next set's
    /// continue where they left off.
    DigitalId, "The number this action was assigned, dense from zero."
}

id_type! {
    /// One two-axis action: a stick, a mouse delta, a pair of triggers read
    /// together.
    ///
    /// Numbered in its own space, so [`AnalogId(0)`](Self) and
    /// [`DigitalId(0)`](crate::DigitalId) are different actions.
    AnalogId, "The number this action was assigned, dense from zero."
}

id_type! {
    /// One tracked pose: a hand, a controller, a headset.
    ///
    /// Numbered in its own space, like the other two. A pose is the one kind
    /// that can be absent while its set is active — a hand outside the tracking
    /// volume has no transform, which is why
    /// [`Input::pose`](crate::Input::pose) returns an `Option`.
    PoseId, "The number this action was assigned, dense from zero."
}

id_type! {
    /// One action set: a mode the game is in, and the set of actions that mean
    /// something while it is.
    ///
    /// Numbered from zero in declaration order. Exactly one set is active at a
    /// time, and [`Input`](crate::Input) answers for the actions of that one.
    SetId, "The number this set was assigned, dense from zero and in declaration order."
}
