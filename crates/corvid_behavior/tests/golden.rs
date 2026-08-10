//! The frozen encodings. **Changing a value in this file is a wire-format
//! break.**
//!
//! Everything this crate defines that goes on the wire implements [`Hash`], and
//! the numbers those implementations absorb are a wire format rather than an
//! implementation detail: a hash trace recorded by today's build is compared
//! against one recorded by yesterday's, and against one recorded on another
//! machine an hour ago. A variant renumbered, a field emitted in a different
//! order, a tag moved behind its payload -- none of those is a compile error,
//! and every one of them shows up as a desync or a refused save rather than as
//! a red test, unless the outputs are written down.
//!
//! So they are written down, as literals. Every other test in this crate
//! compares digests to each other, which catches a digest that stopped
//! distinguishing two things and cannot catch one that distinguishes them
//! differently than it did when the row was recorded. This file is the other
//! half.
//!
//! This is one of the crate's two frozen tables. `tests/wire.rs` is the other,
//! and it freezes the *serialized bytes*, which are a different encoding over
//! the same fixtures: widening an integer moves every byte row there and every
//! digest row here, while renaming a field moves the JSON half of that table and
//! no digest whatsoever. Neither table substitutes for the other, and the
//! fixtures live in `tests/common/vocabulary.rs` so that both are talking about
//! the same values.
//!
//! If a change here is genuinely wanted, it is a new version of the format:
//! bump the crate's major version, reissue every trace recorded under the old
//! one, and say so in the changelog. Regenerating these numbers to make a red
//! test go green is never the right move -- the red test *is* the notification
//! that peers on two builds have stopped agreeing.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::vocabulary::{every_player, every_presence};
use corvid_behavior::{
    AchievementId, ExitCode, LobbyId, PlayerId, PresenceText, ProfileId, RumbleId, SaveSlot,
    StatId, Url,
};
use corvid_hash::{Digest, digest};
use corvid_wire::golden::{DigestRow, check_digests};

/// Every [`Presence`], including the one with no payload.
const GOLDEN_PRESENCE: &[DigestRow<'_>] = &[
    ("Joining(77)", 0x3645_a6c0_57fa_b645),
    ("Active", 0x3178_2188_0dd5_d02b),
    ("Dropped(4)", 0x72ba_b7c5_7634_55f0),
];

/// A [`Player`], whose three fields are emitted in declaration order.
///
/// Every field here holds a different value from every other, which is what
/// makes the order visible: a `Player` that emitted its fields backwards would
/// still digest to something, and would still distinguish two different
/// players, and would still pass every relative test in this crate.
///
/// These two rows were re-recorded when `Player::pose` was deleted -- the field
/// was emitted second, so removing it moved both of them.
const GOLDEN_PLAYERS: &[DigestRow<'_>] = &[
    ("seat 2, dropped at 5, action 7", 0xbe69_4c26_70ec_04ca),
    ("seat 2, active, action 7", 0xc378_a4f7_ba73_193d),
];

/// The identifiers, which absorb their integer and no type tag.
///
/// One row per *width* rather than one per type, because absorbing no type tag
/// is exactly what makes the rest redundant: [`SaveSlot`], [`RumbleId`],
/// [`AchievementId`] and [`StatId`] are all `u16` and all digest to whatever
/// [`PlayerId`] does. Recording them as literals would freeze the same number
/// five times and make a genuine `u16` change look like five breaks. The test
/// below asserts that they do agree, which is the claim this table is standing
/// on and the one that would fail first if a type tag ever appeared.
///
/// The two named [`ExitCode`]s are rows here rather than in the `Command` table
/// because they are the numbers that leave the process: swapping
/// `SUCCESS` and `FAILURE` is a change no relative comparison in this crate can
/// see -- everything else compares the two of them to each other, and a swap
/// leaves every one of those comparisons undisturbed -- and it would ship a game
/// that exits 1 on a clean quit.
const GOLDEN_IDS: &[DigestRow<'_>] = &[
    ("PlayerId(0)", 0xa84f_c15c_a001_a03e),
    ("PlayerId(2)", 0xebbb_4486_c579_7f68),
    ("ProfileId(77)", 0x6915_b1ef_2867_6fa9),
    ("ExitCode(255)", 0xaa46_dd0e_2501_f247),
    ("ExitCode::SUCCESS", 0xad32_a923_0bbf_a127),
    ("ExitCode::FAILURE", 0x79be_4077_29e0_2db1),
    ("LobbyId(u64::MAX)", 0x5cc8_a00e_1392_4cb4),
];

/// The names, which absorb their length and then their bytes and never their
/// padding.
const GOLDEN_NAMES: &[DigestRow<'_>] = &[
    ("PresenceText()", 0x223b_5954_3b79_e2c3),
    ("Url(https://example.invalid)", 0x26b6_2ffd_6962_4188),
];

#[test]
fn every_presence_digests_to_its_recorded_value() {
    let digests: Vec<Digest> = every_presence().iter().map(digest).collect();
    check("Presence", GOLDEN_PRESENCE, &digests);
}

#[test]
fn a_players_three_fields_digest_in_their_recorded_order() {
    let action = 7_u32;
    let digests: Vec<Digest> = every_player(&action).iter().map(digest).collect();
    check("Player", GOLDEN_PLAYERS, &digests);
}

#[test]
fn the_identifiers_digest_to_their_recorded_values() {
    let digests = vec![
        digest(&PlayerId(0)),
        digest(&PlayerId(2)),
        digest(&ProfileId(77)),
        digest(&ExitCode(255)),
        digest(&ExitCode::SUCCESS),
        digest(&ExitCode::FAILURE),
        digest(&LobbyId(u64::MAX)),
    ];
    check("identifiers", GOLDEN_IDS, &digests);

    // Which number each of the two named codes actually is. The table above does
    // catch a swap -- `check` zips it positionally, so trading the two constants
    // moves both rows -- but it catches it as two wire-format breaks, and a
    // reader who came to a red golden test expecting a changed encoding is being
    // sent to the wrong question. These two lines name the failure: the
    // operating system reads the number, and it does not read it symmetrically.
    assert_eq!(ExitCode::SUCCESS, ExitCode(0), "success has to be zero");
    assert_eq!(ExitCode::FAILURE, ExitCode(1));

    // Two identifiers of different kinds holding the same number digest alike,
    // which is the convention and not an accident: what establishes that two
    // peers are reading the same field is the opening's schema, not a tag on
    // every value.
    //
    // Every `u16` identifier this crate defines is here, so the table above
    // holding one row for the width covers all of them. An identifier that grew
    // a type tag would fail here rather than pass quietly by not being in the
    // table.
    let seat = digest(&PlayerId(2));
    assert_eq!(seat, digest(&SaveSlot(2)));
    assert_eq!(seat, digest(&RumbleId(2)));
    assert_eq!(seat, digest(&AchievementId(2)));
    assert_eq!(seat, digest(&StatId(2)));
}

#[test]
fn the_names_digest_to_their_recorded_values() {
    let digests = vec![
        digest(&PresenceText::EMPTY),
        digest(&Url::new("https://example.invalid").unwrap()),
    ];
    check("names", GOLDEN_NAMES, &digests);
}

/// The workspace's digest-table comparison, over this crate's `Digest`.
///
/// `corvid_wire::golden::check_digests` is the comparison itself: it reports
/// every row that moved at once, as paste-ready literals, because a deliberate
/// format change moves every row and an accidental one usually moves a handful.
/// It lived here until three crates had grown their own copy of it and the
/// three had started to drift. What is left is turning a slice of `Digest` into
/// the `u64`s it takes, and a moved row into a failed test.
fn check(what: &str, table: &[DigestRow<'_>], digests: &[Digest]) {
    let bits: Vec<u64> = digests.iter().map(|digest| digest.to_u64()).collect();
    check_digests(what, table, &bits).unwrap();
}
