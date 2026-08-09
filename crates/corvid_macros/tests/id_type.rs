//! What `id_type!` declares, pinned against the claims the README makes for it.
//!
//! A macro that declares a type has an unusual test surface: there is no
//! function to feed edge cases to, so what is worth asserting is the *shape* of
//! what comes out -- the wire form, the digest, the ordering and the display --
//! at the boundaries of each repr rather than in its middle.
//!
//! Two of the assertions are made by this file compiling at all rather than by
//! an `assert!`, and are recorded here so a later reader does not delete them
//! as decoration. The declarations below sit at module scope in an ordinary
//! crate, which is where callers will write them and which the README's
//! doctest -- a function body -- does not exercise. And every one of them carries
//! a doc comment, which is the `$(#[$meta])*` passthrough test: the workspace
//! sets `missing_docs = "deny"`, and that meta is the only route a doc comment
//! has into the declared type, so deleting one stops the build. The field doc
//! is load-bearing in a duller way -- `$field_doc` is a required position in the
//! matcher, so a caller cannot forget it, only pass an empty string.

#![allow(
    clippy::panic_in_result_fn,
    reason = "these tests use ? for the library calls and assert! for the checks"
)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher as _};

use corvid_macros::id_type;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

id_type! {
    /// Which seat, in a session's roster.
    SeatId, u16, "The position in the roster."
}

id_type! {
    /// A second kind of identifier at the same width as [`SeatId`], which is
    /// what makes the digest claim testable: same width, different type.
    SlotId, u16, "The slot."
}

id_type! {
    /// Which account.
    AccountId, u64, "The identifier the platform handed out."
}

id_type! {
    /// A signed repr, so the negative half of `Display` and of the encoding is
    /// covered and not merely assumed to follow from the unsigned half.
    Offset, i8, "How far, and which way.",
}

/// A struct with an identifier in it, to see the encoding in the position it
/// will actually be read from -- a field of a message -- rather than alone.
#[cfg(feature = "serde")]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Seating {
    seat: SeatId,
    account: AccountId,
}

/// `#![no_implicit_prelude]` is the strongest available check on the claim that
/// the expansion names nothing a caller might not have: strip the prelude from
/// a module and the declaration still has to stand up in it.
mod without_the_prelude {
    #![no_implicit_prelude]

    use ::corvid_macros::id_type;

    id_type! {
        /// An identifier declared where `Clone` and `Debug` are not in scope
        /// under those names.
        BareId, u32, "The number."
    }
}

fn digest<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn the_field_is_public_and_display_is_the_integer() {
    assert_eq!(SeatId(3).0, 3);
    assert_eq!(SeatId(3).to_string(), "SeatId(3)");

    // The ends of each repr, where a `Display` written by hand would be most
    // likely to have gone through a narrower intermediate.
    assert_eq!(SeatId(u16::MIN).to_string(), "SeatId(0)");
    assert_eq!(SeatId(u16::MAX).to_string(), "SeatId(65535)");
    assert_eq!(
        AccountId(u64::MAX).to_string(),
        "AccountId(18446744073709551615)"
    );
    assert_eq!(Offset(i8::MIN).to_string(), "Offset(-128)");
    assert_eq!(Offset(-1).to_string(), "Offset(-1)");
    assert_eq!(Offset(i8::MAX).to_string(), "Offset(127)");

    // `Copy`, so passing one does not move it. This is a compile-time claim
    // dressed as a runtime one; it fails to build rather than to assert.
    let seat = SeatId(9);
    let copy = seat;
    assert_eq!(seat, copy);
}

#[test]
fn default_is_the_zero_of_the_repr() {
    assert_eq!(SeatId::default(), SeatId(0));
    assert_eq!(AccountId::default(), AccountId(0));
    assert_eq!(Offset::default(), Offset(0));
}

#[test]
fn ordering_is_the_integer_ordering_including_the_signed_half() {
    assert!(SeatId(0) < SeatId(1));
    assert!(SeatId(u16::MAX) > SeatId(u16::MAX - 1));
    // The one place a newtype over a signed integer could plausibly have been
    // ordered by its unsigned bit pattern instead.
    assert!(Offset(i8::MIN) < Offset(0));
    assert!(Offset(-1) < Offset(0));

    let mut seats = [SeatId(2), SeatId(0), SeatId(1)];
    seats.sort_unstable();
    assert_eq!(seats, [SeatId(0), SeatId(1), SeatId(2)]);
}

#[cfg(feature = "serde")]
#[test]
fn the_encoding_is_the_bare_number() -> Result<(), serde_json::Error> {
    assert_eq!(serde_json::to_string(&SeatId(3))?, "3");
    assert_eq!(serde_json::to_string(&SeatId(u16::MAX))?, "65535");
    assert_eq!(serde_json::to_string(&Offset(i8::MIN))?, "-128");
    // Written out rather than computed, because the failure this pins is the
    // one where a wide identifier acquires a float somewhere and comes back
    // rounded.
    assert_eq!(
        serde_json::to_string(&AccountId(u64::MAX))?,
        "18446744073709551615"
    );

    let seating = Seating {
        seat: SeatId(3),
        account: AccountId(18_446_744_073_709_551_615),
    };
    assert_eq!(
        serde_json::to_string(&seating)?,
        r#"{"seat":3,"account":18446744073709551615}"#
    );
    assert_eq!(
        serde_json::from_str::<Seating>(r#"{"seat":3,"account":18446744073709551615}"#)?,
        seating
    );
    Ok(())
}

#[cfg(feature = "serde")]
#[test]
fn every_16_bit_value_survives_a_json_round_trip() -> Result<(), serde_json::Error> {
    for number in u16::MIN..=u16::MAX {
        let seat = SeatId(number);
        let text = serde_json::to_string(&seat)?;
        assert_eq!(text, number.to_string(), "the encoding grew a wrapper");
        assert_eq!(serde_json::from_str::<SeatId>(&text)?, seat);
    }
    Ok(())
}

#[cfg(feature = "serde")]
#[test]
fn a_wrapper_on_the_wire_is_rejected_and_so_is_an_out_of_range_number() {
    // The three shapes a reader would see if the identifier had been encoded
    // as a struct, as a tuple, or as text.
    assert!(serde_json::from_str::<SeatId>(r#"{"0":3}"#).is_err());
    assert!(serde_json::from_str::<SeatId>("[3]").is_err());
    assert!(serde_json::from_str::<SeatId>(r#""3""#).is_err());
    // The newtype does not widen its repr on the way in: one past `u16::MAX`
    // is an error, not a silent promotion.
    assert!(serde_json::from_str::<SeatId>("65536").is_err());
    assert!(serde_json::from_str::<Offset>("-129").is_err());
}

#[test]
fn the_digest_is_the_integer_and_no_type_tag() {
    // Same width, different kind: the README's claim, and the reason the
    // second `u16` identifier above exists.
    assert_eq!(digest(&SeatId(3)), digest(&SlotId(3)));
    // And it is the bare integer's digest, which is the stronger statement --
    // nothing at all is fed to the hasher besides the number.
    assert_eq!(digest(&SeatId(3)), digest(&3_u16));
    assert_eq!(digest(&AccountId(3)), digest(&3_u64));
    assert_eq!(digest(&Offset(-1)), digest(&-1_i8));

    // `SeatId(3)` and `AccountId(3)` feed the hasher different bytes, because
    // `Hash` for an integer writes its own width and a `u16` is not a `u64`.
    // Nothing is asserted about what comes back out, on purpose: the three
    // above already fix each identifier's digest to its repr's, which is the
    // whole of the claim, and a `!=` between two digests would be a test
    // asserting the absence of a collision -- which no `Hasher` promises, so
    // it would be pinning this hasher's luck rather than this crate's
    // behaviour.
}

#[test]
fn an_identifier_keys_a_map() {
    use std::collections::HashMap;

    // The digest above is deliberately the integer's, so two kinds of the same
    // width land in the same bucket. Nothing is lost by that: an identifier is
    // only ever a key among its own kind, since the map's key type is one of
    // them, and within a kind `Eq` separates the numbers as usual.
    let mut seats = HashMap::new();
    seats.insert(SeatId(3), "third seat");
    assert_eq!(seats.get(&SeatId(3)), Some(&"third seat"));
    assert_eq!(seats.len(), 1);
    seats.insert(SeatId(4), "fourth seat");
    assert_eq!(seats.len(), 2);
}

#[test]
fn a_declaration_survives_a_module_without_the_prelude() {
    let bare = without_the_prelude::BareId(7);
    assert_eq!(bare.0, 7);
    assert_eq!(bare.to_string(), "BareId(7)");
}
