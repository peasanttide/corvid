//! The numbered identifiers, which are all the same type with different names.
//!
//! Each is a newtype over an integer with the field public, because an
//! identifier is a number and hiding it behind an accessor would cost every
//! caller a line and buy nothing. What the newtype buys is that a
//! [`PlayerId`](crate::PlayerId) cannot be passed where a
//! [`ProfileId`](crate::ProfileId) was meant, which is a mistake that would
//! otherwise compile and then send the wrong person an invitation.

/// Declares one of them, with its serde encoding.
///
/// The derived [`Hash`] absorbs the integer and no type tag, which is the
/// convention the rest of the workspace hashes under: what establishes that two
/// peers are reading the same field is the opening's schema, not a tag on every
/// value. Two identifiers of different kinds holding the same number therefore
/// digest alike, and that is fine, because nothing ever hashes one out of
/// context.
macro_rules! id_type {
    (
        $(#[$meta:meta])*
        $name:ident, $repr:ty, $field_doc:literal
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[derive(::serde::Serialize, ::serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(
            #[doc = $field_doc]
            pub $repr,
        );

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.0, f)
            }
        }
    };
}

pub(crate) use id_type;
