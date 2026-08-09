//! What `named_enum!` declares, pinned against the claims the README makes for
//! it.
//!
//! The claim worth testing is the one the macro exists for: `ALL` is generated
//! from the variant list, so it cannot fall behind it. That is not something an
//! `assert!` can show on its own -- a hand-written `ALL` would pass every
//! assertion below on the day it was written, and the macro's whole value is
//! about the day *after* a variant is added. So the assertions here pin the
//! shape (order, length, membership, the names) and the second declaration
//! pins the part a single enum cannot: adding a variant to `Extended` that
//! `Basic` does not have moves `ALL` and `name` together, with nothing written
//! by hand in between.
//!
//! Two assertions are made by this file compiling at all. The declarations sit
//! at module scope in an ordinary crate, which is where callers will write them
//! and which the README's doctest -- a function body -- does not exercise. And
//! every variant carries a doc comment: the workspace sets
//! `missing_docs = "deny"`, and `$(#[$variant_meta])*` is the only route one
//! has into a generated variant, so deleting the passthrough stops the build.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_macros::named_enum;

named_enum! {
    /// Two variants, one of them two words.
    Basic {
        /// The first.
        Closed = "closed",
        /// The second, whose name is not its identifier lowercased.
        TimedOut = "timed out",
    }
}

named_enum! {
    /// `Basic` with a third variant, which is the case the macro is for: the
    /// only edit between the two declarations is the variant itself.
    #[non_exhaustive]
    Extended {
        /// The first.
        Closed = "closed",
        /// The second.
        TimedOut = "timed out",
        /// The third, added without touching an `ALL` anywhere.
        Refused = "refused",
    }
}

#[test]
fn all_is_every_variant_in_declaration_order() {
    assert_eq!(Basic::ALL, [Basic::Closed, Basic::TimedOut]);
    assert_eq!(
        Extended::ALL,
        [Extended::Closed, Extended::TimedOut, Extended::Refused]
    );
}

#[test]
fn a_variant_added_lands_in_all_and_in_name_with_nothing_written_between() {
    // The two declarations differ by one variant and nothing else -- no length,
    // no second list. This is the property a hand-written `ALL` cannot offer
    // and the reason the macro exists.
    assert_eq!(Basic::ALL.len(), 2);
    assert_eq!(Extended::ALL.len(), 3);

    let basic: Vec<&str> = Basic::ALL.iter().map(|it| it.name()).collect();
    let extended: Vec<&str> = Extended::ALL.iter().map(|it| it.name()).collect();
    assert_eq!(basic, ["closed", "timed out"]);
    assert_eq!(extended, ["closed", "timed out", "refused"]);
}

#[test]
fn the_name_is_the_literal_and_not_the_identifier() {
    // The whole reason the names are a required position rather than derived:
    // `TimedOut` is one word and "timed out" is two, and what a person reads in
    // a report is the second.
    assert_eq!(Basic::TimedOut.name(), "timed out");
    assert_eq!(Extended::TimedOut.name(), "timed out");
}

#[test]
fn display_forwards_to_name_for_every_variant() {
    for &variant in Basic::ALL {
        assert_eq!(variant.to_string(), variant.name());
    }
    for &variant in Extended::ALL {
        assert_eq!(variant.to_string(), variant.name());
    }
}

#[test]
fn the_declared_derives_are_there() {
    // Copy and Eq by using a variant twice and comparing; Ord and Hash by
    // sorting and by going into a set. A derive dropped from the expansion
    // fails to compile here rather than at some caller.
    let first = Basic::Closed;
    let same = first;
    assert_eq!(first, same);
    assert!(Basic::Closed < Basic::TimedOut);

    let mut sorted = vec![Extended::Refused, Extended::Closed, Extended::TimedOut];
    sorted.sort_unstable();
    assert_eq!(sorted, Extended::ALL);

    let unique: std::collections::HashSet<Basic> = Basic::ALL.iter().copied().collect();
    assert_eq!(unique.len(), 2);

    assert_eq!(format!("{:?}", Basic::TimedOut), "TimedOut");
}

#[test]
fn name_is_const() {
    // `name` is declared `const fn`, so it can be used where a constant is
    // wanted. This is the assertion: it would not compile otherwise.
    const CLOSED: &str = Basic::Closed.name();
    assert_eq!(CLOSED, "closed");
}

/// The meta passthrough puts `#[non_exhaustive]` on the type, which is only
/// visible from another crate -- so what is checkable here is the other half:
/// that a match inside the declaring crate stays exhaustive without a wildcard.
#[test]
fn a_match_in_the_declaring_crate_needs_no_wildcard() {
    for &variant in Extended::ALL {
        let named = match variant {
            Extended::Closed => "closed",
            Extended::TimedOut => "timed out",
            Extended::Refused => "refused",
        };
        assert_eq!(named, variant.name());
    }
}

/// `#![no_implicit_prelude]` is the strongest available check that the
/// expansion leans on nothing the caller happens to have imported -- the same
/// check `id_type!` gets next door.
mod without_the_prelude {
    #![no_implicit_prelude]

    use ::corvid_macros::named_enum;

    named_enum! {
        /// Declared where nothing is in scope.
        Bare {
            /// The only variant.
            Only = "only",
        }
    }

    #[test]
    fn it_expands_with_nothing_in_scope() {
        ::core::assert_eq!(Bare::ALL.len(), 1);
        ::core::assert_eq!(Bare::Only.name(), "only");
    }
}
