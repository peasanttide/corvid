//! The frozen encoding. **Changing a value in this file is a wire-format
//! break.**
//!
//! A tick is what every recorded thing in this workspace is stamped with. A
//! replay is a tick and a list of actions, a snapshot is a tick and a state, a
//! desync report is two peers naming a tick — so if the eight bytes below became
//! four, every one of those files would still load, and would load at the wrong
//! moment or not at all.
//!
//! Eight bytes rather than four is a real decision and not the default. `Tick`
//! is a `u64` because the crate's own documentation promises saturation is
//! thirty-nine billion years away at fifteen ticks a second, and a `u32` would
//! bring that to nine years. A change of width here is a four-line edit — the
//! field and the three signatures that name it — and once it compiles, every
//! round trip in the workspace stays green, because the writer and the reader
//! would have moved together, and every JSON row stays green, because JSON
//! spells a number the same at every width. This table is what that edit runs
//! into.
//!
//! Three views, then, and each blind where the others see. The byte table is a
//! varint, so it spells a value and never a declared width. The crate's JSON
//! tests write `4` for a `u32` and for a `u64` alike, and so cannot see a width
//! either, but they are the only thing that sees a field renamed — this encoding
//! writes no names. And the digest table at the bottom of this file is where a
//! width shows, because `corvid_hash` absorbs an integer as its declared bytes
//! and injects the count.
//!
//! If a change here is genuinely wanted, it is a new version of the format: bump
//! the crate's major version, reissue every replay recorded under the old one,
//! and say so in the changelog.

#![cfg(feature = "serde")]
#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use core::num::NonZeroU32;

use corvid_time::{Tick, TickRate};
use corvid_wire::golden::{DigestRow, Row, check, check_digests};

/// The tick counter: eight bytes, least significant first.
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

/// The tick rate: four bytes, and no tag for the fact that it cannot be zero.
///
/// `TickRate` wraps a `NonZeroU32`, and the first row is what says that costs
/// nothing on the wire — fifteen is `0f000000` and not a tag byte and then
/// fifteen. It also says a rate is *not* stored as a period: a capture that held
/// the sixty-six million nanoseconds `CRADLE` runs at would look nothing like
/// this, and reading one as the other would produce a rate no player could
/// survive.
const GOLDEN_RATES: &[Row<'_>] = &[
    ("TickRate::CRADLE, fifteen hertz", "0f"),
    ("TickRate::from_hz(0x1234_5678)", "fc78563412"),
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
fn the_tick_rate_encodes_as_it_was_recorded() {
    let fast = NonZeroU32::new(0x1234_5678).unwrap();
    check(
        "TickRate",
        GOLDEN_RATES,
        &[TickRate::CRADLE, TickRate::from_hz(fast)],
    )
    .unwrap();
}

#[test]
fn a_tick_and_a_rate_are_their_numbers_and_nothing_else() {
    // Both are transparent: the bytes of a tick are the bytes of the number in
    // it, with no wrapper and — for the rate — no non-zero tag.
    assert_eq!(
        corvid_wire::encode(&Tick(1)).unwrap(),
        corvid_wire::encode(&1_u64).unwrap(),
    );
    assert_eq!(
        corvid_wire::encode(&TickRate::CRADLE).unwrap(),
        corvid_wire::encode(&15_u32).unwrap(),
    );

    // And a number this small is one byte at either width, which is the shape
    // the early ticks of every capture have. A `Tick` narrowed to a `u32` would
    // write these same bytes and pass every round trip in the crate — what it
    // would move is the digest, and `GOLDEN_MARKS` below is that table.
    assert_eq!(corvid_wire::encode(&Tick(1)).unwrap(), [0x01]);
    assert_eq!(corvid_wire::encode(&TickRate::CRADLE).unwrap(), [0x0f]);
    assert_ne!(
        corvid_hash::digest(&Tick(1)),
        corvid_hash::digest(&TickRate::from_hz(NonZeroU32::new(1).unwrap())),
    );
}

#[test]
fn a_tick_beyond_a_narrower_counter_is_in_the_table() {
    // The row above is only worth having if the number in it is one a narrowed
    // counter could not have held, so that is checked rather than assumed.
    assert!(u32::try_from(0x1234_5678_9abc_def0_u64).is_err());
}

/// What a `Tick` and a `TickRate` digest to under `corvid_hash`'s hasher.
///
/// The third of the three views, and the only one that sees a **width**. The
/// byte table above is a varint, so `Tick(1)` is one byte whether the counter is
/// a `u32` or a `u64`; the crate's JSON tests write `1` at either width too. A
/// hasher absorbs an integer as its declared bytes and injects the count of
/// bytes absorbed, so narrowing `Tick` moves every row here — which matters
/// because a hash trace is what two peers actually compare, and `Tick` is what
/// every row of one is stamped with.
///
/// The small values are the rows that carry the claim: the large ones would move
/// the byte table on their own.
const GOLDEN_MARKS: &[DigestRow<'_>] = &[
    ("Tick::ZERO", 0x7383_3581_a38e_f3cd),
    ("Tick(1)", 0x3178_2188_0dd5_d02b),
    ("Tick(0x1234_5678_9abc_def0)", 0x23a9_aafe_59d6_50f2),
    ("TickRate::CRADLE", 0x1783_4fb1_c92c_3ba5),
    ("TickRate::from_hz(1)", 0xd2ad_74d3_e9bb_9f8b),
];

#[test]
fn a_tick_and_a_rate_digest_as_they_were_recorded() {
    let marks = [
        corvid_hash::digest(&Tick::ZERO),
        corvid_hash::digest(&Tick(1)),
        corvid_hash::digest(&Tick(0x1234_5678_9abc_def0)),
        corvid_hash::digest(&TickRate::CRADLE),
        corvid_hash::digest(&TickRate::from_hz(NonZeroU32::new(1).unwrap())),
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
