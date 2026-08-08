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
/// The type itself is `pub` too, and that is not a parameter. A `$vis:vis`
/// position would have made it one, and there is nothing yet to turn it with:
/// an identifier no other module can name is an integer with extra steps.
///
/// The expansion names `::serde`, so a crate calling this depends on `serde`
/// with `derive`. This crate does not.
///
/// The derived [`Hash`] absorbs the integer and no type tag, which is the
/// convention the rest of the workspace hashes under: what establishes that two
/// peers are reading the same field is the opening's schema, not a tag on every
/// value. An identifier therefore digests exactly as the bare integer inside it
/// does, and two identifiers *of the same width* holding the same number digest
/// alike; that is fine, because nothing ever hashes one out of context. Two of
/// different widths do not, but that is [`Hash`] for an integer feeding its own
/// width to the hasher rather than a tag reappearing, so it is not a distinction
/// to lean on: widen one of the two reprs and the digests meet again.
#[macro_export]
macro_rules! id_type {
    (
        $(#[$meta:meta])*
        $name:ident, $repr:ty, $field_doc:literal $(,)?
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
