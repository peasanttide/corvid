//! The frozen encoding. **Changing a value in this file is a wire-format
//! break.**
//!
//! A tick is what every recorded thing in this workspace is stamped with. A
//! replay is a tick and a list of actions, a snapshot is a tick and a state, a
//! desync report is two peers naming a tick -- so if the eight bytes below became
//! four, every one of those files would still load, and would load at the wrong
//! moment or not at all.
//!
//! Eight bytes rather than four is a real decision and not the default. `Tick`
//! is a `u64` because the crate's own documentation promises saturation is
//! thirty-nine billion years away at fifteen ticks a second, and a `u32` would
//! bring that to nine years. A change of width here is a handful of lines -- the
//! field and the signatures that name it -- and once it compiles, every
//! round trip in the workspace stays green, because the writer and the reader
//! would have moved together, and every JSON row stays green, because JSON
//! spells a number the same at every width. This table is what that edit runs
//! into.
//!
//! Three views, then, and each blind where the others see. The byte table is a
//! varint, so it spells a value and never a declared width. The crate's JSON
//! tests write `4` for a `u32` and for a `u64` alike, and so cannot see a width
//! either, but they are the only thing that sees a field renamed -- this encoding
//! writes no names. And the digest table at the bottom of this file is where a
//! width shows, because `corvid_hash` absorbs an integer as its declared bytes
//! and injects the count.
//!
//! If a change here is genuinely wanted, it is a new version of the format: bump
//! the crate's major version and reissue every replay recorded under the old
//! one.

#![cfg(feature = "serde")]
#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use core::num::NonZeroU32;

use corvid_time::{Tick, TickSpan};
use corvid_wire::golden::{DigestRow, Row, check, check_digests};

/// The tick counter: a `u64` field, written as a varint.
///
/// The declared width and the encoded length are different numbers here, which
/// is the whole reason this table is worth freezing. `Tick(1)` is one byte, not
/// eight -- the encoding spells the value and never the width it was declared
/// at, so nothing in these rows would move if the field narrowed to a `u32`
/// until a value arrived that a `u32` could not hold. The digest table below is
/// what sees the width.
///
/// The last two rows are the ones a narrowing runs into. A tick of
/// `0x1234_5678_9abc_def0` is a number a `u32` cannot hold, so the row is a
/// statement that the counter is wide enough to reach it; `u64::MAX` pins the
/// top of the range and, with the zero row, pins that nothing is offset or
/// biased on the way out.
const GOLDEN_TICKS: &[Row<'_>] = &[
    ("Tick::ZERO", "00"),
    ("Tick(1)", "01"),
    ("Tick(0x1234_5678_9abc_def0)", "fdf0debc9a78563412"),
    ("Tick(u64::MAX)", "fdffffffffffffffff"),
];

/// The tick span: a varint of nanoseconds, and no tag for the fact that it
/// cannot be zero.
///
/// What a span holds is nanoseconds, so `CRADLE` is the five bytes of
/// 66 666 666.
///
/// The second row is the one that says why the span is what is stored: a
/// three-hundred-megahertz rate truncates to a three-nanosecond span, and the
/// stored value is that span exactly rather than a rate the simulation would
/// have had to re-derive it from. The third is a span no whole rate names.
const GOLDEN_RATES: &[Row<'_>] = &[
    (
        "TickSpan::CRADLE, sixty-six million nanoseconds",
        "fcaa40f903",
    ),
    ("TickSpan::from_hz(0x1234_5678), three nanoseconds", "03"),
    (
        "TickSpan::from_nanos(13_888_888), a 72 Hz headset",
        "fc78edd300",
    ),
];

#[test]
fn the_tick_encodes_as_it_was_recorded() {
    check(
        "Tick",
        GOLDEN_TICKS,
        &[
            Tick::ZERO,
            Tick(1),
            Tick(0x1234_5678_9abc_def0),
            Tick(u64::MAX),
        ],
    )
    .unwrap();
}

#[test]
fn the_tick_span_encodes_as_it_was_recorded() {
    let fast = NonZeroU32::new(0x1234_5678).unwrap();
    let headset = NonZeroU32::new(13_888_888).unwrap();
    check(
        "TickSpan",
        GOLDEN_RATES,
        &[
            TickSpan::CRADLE,
            TickSpan::from_hz(fast),
            TickSpan::from_nanos(headset),
        ],
    )
    .unwrap();
}

#[test]
fn a_tick_and_a_span_are_their_numbers_and_nothing_else() {
    // Both are transparent: the bytes of a tick are the bytes of the number in
    // it, with no wrapper and -- for the span -- no non-zero tag.
    assert_eq!(
        corvid_wire::encode(&Tick(1)).unwrap(),
        corvid_wire::encode(&1_u64).unwrap(),
    );
    assert_eq!(
        corvid_wire::encode(&TickSpan::CRADLE).unwrap(),
        corvid_wire::encode(&66_666_666_u32).unwrap(),
    );

    // And a number this small is one byte at either width, which is the shape
    // the early ticks of every capture have. A `Tick` narrowed to a `u32` would
    // write these same bytes and pass every round trip in the crate -- what it
    // would move is the digest, and `GOLDEN_MARKS` below is that table.
    assert_eq!(corvid_wire::encode(&Tick(1)).unwrap(), [0x01]);
    // A one-nanosecond span writes the same one byte, because a varint spells a
    // value and never the width it was declared at. The digests differ, and the
    // reason is the whole argument for keeping a digest table beside this one: a
    // tick is a `u64` and a span is a `u32`, and `corvid_hash` absorbs an
    // integer as its declared bytes and injects the count of them.
    let shortest = TickSpan::from_nanos(NonZeroU32::MIN);
    assert_eq!(corvid_wire::encode(&shortest).unwrap(), [0x01]);
    assert_ne!(
        corvid_hash::digest(&Tick(1)),
        corvid_hash::digest(&shortest),
        "a tick and a span write one byte each and must still digest apart",
    );
}

#[test]
fn a_tick_beyond_a_narrower_counter_is_in_the_table() {
    // The row above is only worth having if the number in it is one a narrowed
    // counter could not have held, so that is checked rather than assumed.
    assert!(u32::try_from(0x1234_5678_9abc_def0_u64).is_err());
}

/// What a `Tick` and a `TickSpan` digest to under `corvid_hash`'s hasher.
///
/// The third of the three views the module documentation sets out, and the only
/// one that sees a **width**: narrowing `Tick` moves every row here, which
/// matters because a hash trace is what two peers actually compare and `Tick` is
/// what every row of one is stamped with.
///
/// The small values are the rows that carry the claim: the large ones would move
/// the byte table on their own.
///
/// The two span rows are digests of a `u32`, which is what a span is. Their
/// bytes above are unchanged from when it was a `u64` -- a varint spells the
/// value and never the width -- so these two rows are the only record in the
/// crate that the field ever narrowed. That is the argument for keeping this
/// table, made by the table.
const GOLDEN_MARKS: &[DigestRow<'_>] = &[
    ("Tick::ZERO", 0x7383_3581_a38e_f3cd),
    ("Tick(1)", 0x3178_2188_0dd5_d02b),
    ("Tick(0x1234_5678_9abc_def0)", 0x23a9_aafe_59d6_50f2),
    ("TickSpan::CRADLE", 0xeca4_fcde_ac6a_f52a),
    (
        "TickSpan::from_hz(1), a one-second span",
        0xe363_5a79_a168_86a8,
    ),
];

#[test]
fn a_tick_and_a_rate_digest_as_they_were_recorded() {
    let marks = [
        corvid_hash::digest(&Tick::ZERO),
        corvid_hash::digest(&Tick(1)),
        corvid_hash::digest(&Tick(0x1234_5678_9abc_def0)),
        corvid_hash::digest(&TickSpan::CRADLE),
        corvid_hash::digest(&TickSpan::from_hz(NonZeroU32::new(1).unwrap())),
    ]
    .map(corvid_hash::Digest::to_u64);
    check_digests("the clock's types", GOLDEN_MARKS, &marks).unwrap();
}

#[test]
fn narrowing_the_counter_would_move_every_mark_and_no_byte() {
    // The claim the table above rests on, said once without the table. One is
    // one byte under this encoding at either width, and two different digests.
    assert_eq!(corvid_wire::encode(&1_u32).unwrap(), [0x01]);
    assert_eq!(corvid_wire::encode(&1_u64).unwrap(), [0x01]);
    assert_ne!(corvid_hash::digest(&1_u32), corvid_hash::digest(&1_u64));
}

/// A count of ticks, which is a `u64` behind the same transparent attribute a
/// [`Tick`] carries.
///
/// It has a wire format whether or not anything writes one down today, and an
/// encoding nobody froze is one that moves without saying so. `Ticks(30)` and
/// `Tick(30)` are the same bytes and the same digest, because the two are the
/// same integer -- the distinction they exist to draw is in the type and never on
/// the wire, which is exactly the sort of thing worth having written down.
const GOLDEN_COUNTS: &[Row<'_>] = &[
    ("Ticks::NONE", "00"),
    ("Ticks(30)", "1e"),
    ("Ticks(u64::MAX)", "fdffffffffffffffff"),
];

#[test]
fn a_count_of_ticks_encodes_as_they_were_recorded() {
    use corvid_time::Ticks;

    check(
        "Ticks",
        GOLDEN_COUNTS,
        &[Ticks::NONE, Ticks(30), Ticks(u64::MAX)],
    )
    .unwrap();

    // The same bytes as the tick of that number, which is the claim the type's
    // own documentation makes about itself from the other direction.
    assert_eq!(
        corvid_wire::encode(&Ticks(30)).unwrap(),
        corvid_wire::encode(&Tick(30)).unwrap(),
    );
}
