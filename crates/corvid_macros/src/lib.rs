#![doc = include_str!("../README.md")]
#![no_std]

/// Declares a numbered identifier: a newtype over an integer, with its serde
/// encoding and its [`Display`](core::fmt::Display).
///
/// The field is public, because an identifier is a number and hiding it behind
/// an accessor would cost every caller a line and buy nothing. What the newtype
/// buys is that one kind of identifier cannot be passed where another was
/// meant — a mistake that would otherwise compile and then send the wrong
/// person an invitation.
///
/// The expansion names `::serde`, so a crate calling this depends on `serde`
/// with `derive`. This crate does not.
///
/// The derived [`Hash`] absorbs the integer and no type tag, which is the
/// convention the rest of the workspace hashes under: what establishes that two
/// peers are reading the same field is the opening's schema, not a tag on every
/// value. Two identifiers of different kinds holding the same number therefore
/// digest alike, and that is fine, because nothing ever hashes one out of
/// context.
#[macro_export]
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
