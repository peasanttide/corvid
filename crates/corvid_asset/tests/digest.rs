//! The number two peers compare before they simulate a tick.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

use corvid_asset::{Manifest, Pack, PackId, PackStamp, Stack};
use corvid_files::Memory;

fn id(text: &str) -> PackId {
    PackId::new(text).expect("the identifiers in these tests are short")
}

fn pack(name: &str, version: u32) -> Pack {
    Pack::new(Manifest::new(id(name), name, version), Memory::new())
}

fn stack(packs: &[(&str, u32)]) -> Stack {
    Stack::mount(
        packs
            .iter()
            .map(|(name, version)| pack(name, *version))
            .collect(),
    )
    .expect("nothing in these tests requires anything")
}

/// The same packs the other way round are not the same game.
///
/// A stack is a list: swapping two packs swaps which one's copy of a shared
/// path wins, so two peers who mounted the same mods in different orders would
/// load different content and have to find that out from the digest.
#[test]
fn reordering_two_packs_changes_the_digest() {
    let one_way = stack(&[("terminus", 1), ("riverside", 1)]);
    let the_other = stack(&[("riverside", 1), ("terminus", 1)]);

    assert_eq!(one_way.stamps().len(), the_other.stamps().len());
    assert_ne!(one_way.digest(), the_other.digest());
}

/// A peer with one mod the others do not have disagrees at seating.
#[test]
fn adding_a_pack_changes_the_digest() {
    let plain = stack(&[("terminus", 1)]);
    let modded = stack(&[("terminus", 1), ("weather", 1)]);

    assert_ne!(plain.digest(), modded.digest());
    assert_ne!(
        Stack::new().digest(),
        plain.digest(),
        "an empty stack is not the same set as a stack of one",
    );
}

/// A pack that shipped a fix and turned its version over is a different set.
#[test]
fn a_new_version_of_one_pack_changes_the_digest() {
    assert_ne!(
        stack(&[("terminus", 1)]).digest(),
        stack(&[("terminus", 2)]).digest(),
    );
}

/// The same set mounted twice gives the same answer, in this process and in any
/// other.
///
/// The frozen value is what makes the second half of that a test rather than a
/// hope. Two runs in one process would agree even if the digest were seeded
/// from an address or from the clock; a literal here is the digest a peer
/// computed when this test was written, and it has to still be the digest a
/// peer computes on another machine next year, because that is the only reason
/// comparing the number is worth anything.
#[test]
fn the_same_set_digests_the_same_in_any_process() {
    let once = stack(&[("terminus", 1), ("weather", 3), ("riverside", 12)]);
    let again = stack(&[("terminus", 1), ("weather", 3), ("riverside", 12)]);

    assert_eq!(once.digest(), again.digest());
    assert_eq!(once.digest().to_string(), "e52bd2451731eebd");
}

/// What a game puts in its rules, and it is a list rather than a set.
#[test]
fn stamps_are_the_identifiers_and_versions_in_mount_order() {
    let stack = stack(&[("terminus", 1), ("riverside", 12)]);

    assert_eq!(
        stack.stamps(),
        [
            PackStamp {
                id: id("terminus"),
                version: 1,
            },
            PackStamp {
                id: id("riverside"),
                version: 12,
            },
        ],
    );
}
