#![doc = include_str!("../README.md")]
#![no_std]

/// Declares a numbered identifier: a newtype over an integer, with its serde
/// encoding and its [`Display`](core::fmt::Display).
///
/// The `Display` is `TypeName(number)` rather than the bare number. Which kind
/// of identifier a number is, is the thing the newtype exists to keep straight,
/// so a log line that has lost it has lost the point -- and a report naming two
/// identifiers is unreadable without it. The encoding is unaffected: that is
/// `#[serde(transparent)]` and stays the bare number.
///
/// The field is public, because an identifier is a number and hiding it behind
/// an accessor would cost every caller a line and buy nothing. What the newtype
/// buys is that one kind of identifier cannot be passed where another was
/// meant -- a mistake that would otherwise compile and then send the wrong
/// person an invitation.
///
/// The type itself is `pub` too, and that is not a parameter. A `$vis:vis`
/// position would have made it one, and there is nothing yet to turn it with:
/// an identifier no other module can name is an integer with extra steps.
///
/// The encoding is behind the calling crate's `serde` feature, which that
/// crate therefore has to declare: the expansion is `cfg_attr`, and a `cfg` is
/// read where it expands rather than where it was written. With the feature on,
/// the caller supplies `serde` with `derive` and the identifier encodes as the
/// bare number under `#[serde(transparent)]`. With it off, nothing here names
/// `serde` at all. This crate depends on it either way not at all.
///
/// The derived [`Hash`] absorbs the integer and no type tag, which is the
/// convention the rest of the workspace hashes under: what establishes that two
/// peers are reading the same field is the opening's schema, not a tag on every
/// value. An identifier therefore digests exactly as the bare integer inside it
/// does, and two identifiers *of the same width* holding the same number digest
/// alike; that is fine, because nothing ever hashes one out of context. Two of
/// different widths feed the hasher *different bytes*, because [`Hash`] for an
/// integer writes its own width -- but that is a statement about what goes in
/// rather than a promise about what comes out, and no [`Hasher`] undertakes to
/// keep two inputs apart. It is not a distinction to lean on in either case:
/// widen one of the two reprs and even the input is the same again.
///
/// [`Hasher`]: core::hash::Hasher
#[macro_export]
macro_rules! id_type {
    (
        $(#[$meta:meta])*
        $name:ident, $repr:ty, $field_doc:literal $(,)?
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
            pub $repr,
        );

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::write!(f, "{}({})", ::core::stringify!($name), self.0)
            }
        }
    };
}

/// Declares an enumeration whose variants each have a name a person reads.
///
/// The variants, an `ALL` of them, a `name` returning the one for a variant,
/// and the [`Display`](core::fmt::Display) that forwards to it.
///
/// `ALL` is the reason this is a macro rather than a convention. Written out by
/// hand beside the variants it is a second list to keep level with the first,
/// and nothing in Rust makes a hand-written array grow when a variant is added
/// -- not even in the declaring crate, and a `#[non_exhaustive]` enum cannot be
/// matched exhaustively outside it at all. Generated from the variant list, the
/// two cannot disagree.
///
/// The names are literals rather than derived from the identifiers, because
/// what they are for is a report a person reads: `TimedOut` is one variant and
/// `"timed out"` is two words, and a `Display` that answered the identifier
/// would be answering the wrong thing.
///
/// Nothing about the encoding is declared here, so unlike [`id_type!`] this
/// expansion names no `::serde` and a caller needs none.
///
/// ```
/// use corvid_macros::named_enum;
///
/// named_enum! {
///     /// Why a peer went away.
///     #[non_exhaustive]
///     Parting {
///         /// The other end said goodbye.
///         Closed = "closed",
///         /// It stopped answering.
///         TimedOut = "timed out",
///     }
/// }
///
/// assert_eq!(Parting::ALL, [Parting::Closed, Parting::TimedOut]);
/// assert_eq!(Parting::TimedOut.name(), "timed out");
/// assert_eq!(Parting::TimedOut.to_string(), "timed out");
/// ```
#[macro_export]
macro_rules! named_enum {
    (
        $(#[$meta:meta])*
        $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $label:literal
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )*
        }

        impl $name {
            /// Every variant, in declaration order.
            ///
            /// Generated from the same list the variants are, so it cannot fall
            /// behind them.
            ///
            /// A slice rather than an array, because an array's length is part
            /// of its type: `[Self; 4]` becoming `[Self; 5]` would break every
            /// caller that wrote the length down, which is exactly what
            /// `#[non_exhaustive]` promises adding a variant will not do.
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];

            /// What this is called in a report.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $label,)*
                }
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(self.name())
            }
        }
    };
}
