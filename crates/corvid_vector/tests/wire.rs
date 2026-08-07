//! The frozen encoding. **Changing a value in this file is a wire-format
//! break.**
//!
//! A position is the single most common thing in a snapshot, and these four
//! types are what one is. So these rows are a large share of the bytes of every
//! save file and every state transfer this workspace will ever write, and every
//! one of them comes out of a derive that no line of source constrains.
//!
//! Three things about the encoding are frozen here, and a JSON table can see
//! none of them.
//!
//! The first is that a point is its three components and *no count*. An array is
//! written as a fixed-size tuple, so a row is three components and nothing else
//! — and JSON writes `[1,2,3]` whether the field is `[i32; 3]` or a `Vec<i32>`,
//! so a change that started writing a length would be invisible to it, invisible
//! to every round trip, and would add a byte to every position in every snapshot
//! in the workspace.
//!
//! The second is each component's *value*, spelled as a varint, which is where
//! most of what a capture means lives. What a byte row here does not carry is the
//! component's width. A `GlobalPoint` is three `i32`s and a `GlobalFinePoint` is
//! three `i64`s, and the two write the same bytes for the same small
//! coordinates. The component types are strongly enough tied to their arithmetic
//! that today an edit swapping them does not compile — but the width is
//! `corvid_fixed`'s to change, and if a widening ever does arrive it is silent
//! here: a round trip stays green because the writer and the reader move
//! together, a JSON row stays green because JSON spells a number the same at
//! every width, and these rows stay green because a varint does too. The digest
//! is what moves, and `tests/determinism.rs` is that table.
//!
//! The third is the byte order, which is little-endian on every target — a
//! capture written on an aarch64 laptop is read by an x86-64 server, and neither
//! a round trip nor a JSON row can tell the two machines apart.
//!
//! The component order is the one thing the two tables share, and there it is
//! the JSON one that is unconditional; the blind spot the bytes have is named in
//! the last test below.
//!
//! If a change here is genuinely wanted, it is a new version of the format: bump
//! the crate's major version, reissue every capture recorded under the old one,
//! and say so in the changelog.

#![cfg(feature = "serde")]
#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_fixed::{I16F16, I24F8, I48F16, Signed8, Signed32};
use corvid_vector::{Direction, FinePoint, GlobalFinePoint, GlobalPoint, OctDirection};
use corvid_wire::golden::{Row, check};

/// One of every point type, at a value whose three components differ and whose
/// last one is negative.
///
/// Components that differed only in sign, or that were all the same number,
/// would leave a permuted encoding looking exactly like this one. The `1, 2, -3`
/// pattern is the cheapest fixture that makes an order visible and a width
/// visible at once: the negative component's sign extension is what says how
/// many bytes the component has.
const GOLDEN_POINTS: &[Row<'_>] = &[
    ("GlobalPoint, bits 1, 2, -3", "020405"),
    ("FinePoint, bits 1, 2, -3", "020405"),
    ("GlobalFinePoint, bits 1, 2, -3", "020405"),
    ("Direction, bits 1, 2, -3", "020405"),
];

/// A `GlobalPoint` whose first component is not a small number.
///
/// The row above is three components that a varint would have written in one
/// byte each. This one holds a value that uses its width, so the row is also a
/// statement about where the *second* component starts — which is the thing a
/// reader on the other end of a capture has to get right.
const GOLDEN_WIDE: &[Row<'_>] = &[("GlobalPoint, bits 0x1234_5678, 0, 0", "fcf0ac68240000")];

#[test]
fn every_point_encodes_as_it_was_recorded() {
    check(
        "GlobalPoint",
        &GOLDEN_POINTS[..1],
        &[GlobalPoint::new(
            I24F8::from_bits(1),
            I24F8::from_bits(2),
            I24F8::from_bits(-3),
        )],
    )
    .unwrap();
    check(
        "FinePoint",
        &GOLDEN_POINTS[1..2],
        &[FinePoint::new(
            I16F16::from_bits(1),
            I16F16::from_bits(2),
            I16F16::from_bits(-3),
        )],
    )
    .unwrap();
    check(
        "GlobalFinePoint",
        &GOLDEN_POINTS[2..3],
        &[GlobalFinePoint::new(
            I48F16::from_bits(1),
            I48F16::from_bits(2),
            I48F16::from_bits(-3),
        )],
    )
    .unwrap();
    check(
        "Direction",
        &GOLDEN_POINTS[3..],
        &[Direction::from_array([
            Signed32::from_bits(1),
            Signed32::from_bits(2),
            Signed32::from_bits(-3),
        ])],
    )
    .unwrap();
}

/// The packed normal, which is a **mesh vertex format** and therefore the one
/// type here whose bytes a GPU reads directly.
///
/// Two `i8` and no more: it is `wgpu`'s `Snorm8x2`, and a vertex buffer built
/// from these is handed to a pipeline that was told the attribute is two bytes
/// wide. A change that added a discriminant, a length, or a third component
/// would not merely break a capture — it would silently misread every vertex
/// after the first.
///
/// `40, -3` rather than a symmetric pair for the reason the point rows use
/// `1, 2, -3`: two components that differed only in sign would leave a swapped
/// `u` and `v` looking exactly like this row.
const GOLDEN_OCT: &[Row<'_>] = &[
    ("OctDirection, bits 40, -3", "28fd"),
    ("OctDirection::UP, the zero pattern", "0000"),
];

#[test]
fn the_packed_normal_encodes_as_it_was_recorded() {
    check(
        "OctDirection",
        GOLDEN_OCT,
        &[
            OctDirection::new(Signed8::from_bits(40), Signed8::from_bits(-3)),
            OctDirection::UP,
        ],
    )
    .unwrap();
}

#[test]
fn a_packed_normal_is_two_bytes_and_the_pair_is_not_a_sequence() {
    // Sixteen bits is the whole claim the type makes, and it is the claim a
    // derive could take away without a round trip noticing: an encoding that
    // wrote the two components as a sequence would put a `u64` count in front of
    // them and make every vertex normal ten bytes instead of two.
    let normal = OctDirection::new(Signed8::from_bits(40), Signed8::from_bits(-3));
    assert_eq!(corvid_wire::encode(&normal).unwrap().len(), 2);
    assert_eq!(
        corvid_wire::encode(&normal).unwrap(),
        corvid_wire::encode(&(40_i8, -3_i8)).unwrap(),
    );

    // And it is the two components in that order, which the fixture can say
    // because they differ.
    assert_ne!(
        corvid_wire::encode(&normal).unwrap(),
        corvid_wire::encode(&OctDirection::new(
            Signed8::from_bits(-3),
            Signed8::from_bits(40)
        ))
        .unwrap(),
    );
}

#[test]
fn a_recorded_normal_still_decodes_to_the_direction_it_was_recorded_for() {
    // The bytes are frozen above, and what a capture actually depends on is the
    // *direction* they name. That is a second promise: the codec could be
    // rewritten without any of the rows above moving, and a mesh recorded under
    // the old one would then be shaded wrong.
    let recorded = corvid_wire::decode::<OctDirection>(&[0x28, 0xfd]).unwrap();
    let [x, y, z] = recorded.decode().to_array();
    assert_eq!(
        [x.to_bits(), y.to_bits(), z.to_bits()],
        [922_795_723, -69_209_679, 1_937_871_019],
    );
}

#[test]
fn a_component_that_uses_its_width_still_lands_where_it_was_recorded() {
    check(
        "GlobalPoint",
        GOLDEN_WIDE,
        &[GlobalPoint::new(
            I24F8::from_bits(0x1234_5678),
            I24F8::ZERO,
            I24F8::ZERO,
        )],
    )
    .unwrap();
}

#[test]
fn a_point_is_its_components_and_no_count() {
    // A point is a fixed-size array, so it writes no count: three components and
    // nothing in front of them. An encoding that treated the array as a sequence
    // would have written a count first and made this four bytes rather than
    // three — a change invisible to every round trip and to every JSON table,
    // and one byte on every position in a snapshot.
    let point = GlobalPoint::new(I24F8::from_bits(1), I24F8::from_bits(2), I24F8::ZERO);
    assert_eq!(corvid_wire::encode(&point).unwrap().len(), 3);
    assert_eq!(
        corvid_wire::encode(&point).unwrap(),
        corvid_wire::encode(&(1_i32, 2_i32, 0_i32)).unwrap(),
    );
    // And the count an array does not pay, shown as the difference against the
    // list of the same three numbers.
    assert_eq!(corvid_wire::encode(&vec![1_i32, 2, 0]).unwrap().len(), 4);
}

#[test]
fn three_points_of_the_same_width_are_one_byte_string() {
    // `GlobalPoint`, `FinePoint` and `Direction` are three different types with
    // three different meanings, and a capture cannot tell which one wrote it.
    // That is the convention — what says which field is which is the schema on
    // both peers, not a tag on every value — but it is also the ceiling on what
    // the rows above can catch: a field that changed from one to another is a
    // reinterpretation of unchanged bytes, and no table in this workspace sees
    // it.
    let global = GlobalPoint::new(
        I24F8::from_bits(1),
        I24F8::from_bits(2),
        I24F8::from_bits(-3),
    );
    let fine = FinePoint::new(
        I16F16::from_bits(1),
        I16F16::from_bits(2),
        I16F16::from_bits(-3),
    );
    assert_eq!(
        corvid_wire::encode(&global).unwrap(),
        corvid_wire::encode(&fine).unwrap(),
    );
}

#[test]
fn a_permuted_component_order_is_visible_only_because_the_fixture_says_so() {
    // The rows above catch a point that started writing `z, y, x`, and they
    // catch it only because their three components hold three different
    // numbers. This is the line that keeps that property from being edited away
    // by somebody tidying the fixture: written from a point whose components are
    // equal, every row above would survive the permutation unchanged.
    let ordered = GlobalPoint::new(
        I24F8::from_bits(1),
        I24F8::from_bits(2),
        I24F8::from_bits(-3),
    );
    let permuted = GlobalPoint::new(
        I24F8::from_bits(-3),
        I24F8::from_bits(2),
        I24F8::from_bits(1),
    );
    assert_ne!(
        corvid_wire::encode(&ordered).unwrap(),
        corvid_wire::encode(&permuted).unwrap(),
    );

    // And what a fixture of equal components would have looked like: three
    // identical groups, which every permutation of `x, y, z` maps to itself. A
    // row recorded from this value says nothing about order at all.
    let flat = corvid_wire::encode(&GlobalPoint::splat(I24F8::from_bits(1))).unwrap();
    assert_eq!(flat, [0x02, 0x02, 0x02]);
}

#[test]
fn widening_a_component_moves_the_digest_and_not_the_bytes() {
    // The claim this file's header rests on, checked against the two types that
    // differ by exactly that change: `GlobalPoint` and `GlobalFinePoint` are the
    // same three components at two widths, holding the same three numbers.
    let narrow = corvid_wire::encode(&GlobalPoint::new(
        I24F8::from_bits(1),
        I24F8::from_bits(2),
        I24F8::from_bits(-3),
    ))
    .unwrap();
    let wide = corvid_wire::encode(&GlobalFinePoint::new(
        I48F16::from_bits(1),
        I48F16::from_bits(2),
        I48F16::from_bits(-3),
    ))
    .unwrap();

    // The same three bytes. A varint carries each component's value and not the
    // width it was declared at, so every byte row in this file survives the
    // widening — which is why the claim in the header belongs to the digest.
    assert_eq!(narrow, [0x02, 0x04, 0x05]);
    assert_eq!(wide, narrow);

    // The digest is where it shows: `corvid_hash` absorbs each component as its
    // declared bytes and injects the total count, so twelve bytes of components
    // and twenty-four answer differently.
    assert_ne!(
        corvid_hash::digest(&GlobalPoint::new(
            I24F8::from_bits(1),
            I24F8::from_bits(2),
            I24F8::from_bits(-3),
        )),
        corvid_hash::digest(&GlobalFinePoint::new(
            I48F16::from_bits(1),
            I48F16::from_bits(2),
            I48F16::from_bits(-3),
        )),
    );
}
