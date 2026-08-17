//! What a mounted stack answers: which pack a path comes from, what the whole
//! set contains, and what it refuses.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

use corvid_asset::{Manifest, Pack, PackId, Stack};
use corvid_files::{Memory, Source};

fn id(text: &str) -> PackId {
    PackId::new(text).expect("the identifiers in these tests are short")
}

fn pack(name: &str, files: &[(&str, &str)]) -> Pack {
    Pack::new(
        Manifest::new(id(name), name, 1),
        files
            .iter()
            .map(|(path, body)| (*path, body.as_bytes().to_vec()))
            .collect::<Memory>(),
    )
}

/// Three packs, and the file each path ends up meaning.
///
/// This is the whole override rule: the top pack's `oak` wins, the middle
/// pack's `lead` wins over the bottom's, and the bottom's `stone` survives
/// because nobody above it said anything about stone. There is no merge and no
/// annotation -- a pack overrides by using the path, and that is the entire
/// vocabulary.
#[test]
fn the_topmost_pack_holding_a_path_is_the_one_that_answers() {
    let stack = Stack::mount(vec![
        pack(
            "base",
            &[
                ("materials/oak.toml", "base oak"),
                ("materials/lead.toml", "base lead"),
                ("materials/stone.toml", "base stone"),
            ],
        ),
        pack("weather", &[("materials/lead.toml", "weathered lead")]),
        pack("level", &[("materials/oak.toml", "seasoned oak")]),
    ])
    .expect("nothing requires anything");

    assert_eq!(stack.read("materials/oak.toml").unwrap(), *b"seasoned oak");
    assert_eq!(
        stack.read("materials/lead.toml").unwrap(),
        *b"weathered lead"
    );
    assert_eq!(
        stack.read("materials/stone.toml").unwrap(),
        *b"base stone",
        "a path nothing above overrode still reads out of the bottom",
    );
    assert!(stack.read("materials/glass.toml").is_err());
}

/// Which pack won, which is the question a person has when a material looks
/// wrong.
#[test]
fn provider_names_the_pack_a_path_came_from() {
    let stack = Stack::mount(vec![
        pack("base", &[("materials/oak.toml", "base oak")]),
        pack("level", &[("materials/oak.toml", "seasoned oak")]),
    ])
    .expect("nothing requires anything");

    let winner = stack.provider("materials/oak.toml").expect("both hold it");
    assert_eq!(winner.manifest().id, id("level"));
    assert!(stack.provider("materials/glass.toml").is_none());
}

/// The listing is the union, each path once, in sorted order.
///
/// The three packs are mounted in an order that is neither the sorted order of
/// their paths nor the reverse of it, so a listing that leaked mount order
/// would be visible here. `zinc` is defined twice on purpose: an overridden
/// path is one file, and a caller told about it twice would load it twice and
/// keep the loser.
#[test]
fn listing_unions_de_duplicates_and_sorts() {
    let stack = Stack::mount(vec![
        pack(
            "base",
            &[("materials/zinc.toml", "base zinc"), ("props/cart.bin", "")],
        ),
        pack(
            "weather",
            &[
                ("materials/oak.toml", ""),
                ("materials/zinc.toml", "rusted"),
            ],
        ),
        pack("level", &[("levels/riverside.toml", "")]),
    ])
    .expect("nothing requires anything");

    assert_eq!(
        stack.list().unwrap(),
        [
            "levels/riverside.toml",
            "materials/oak.toml",
            "materials/zinc.toml",
            "props/cart.bin",
        ],
    );
}

/// A stack holding no packs is not a failure, it is a stack holding no packs.
#[test]
fn an_empty_stack_lists_nothing_and_reads_nothing() {
    let stack = Stack::new();
    assert!(stack.is_empty());
    assert_eq!(stack.len(), 0);
    assert_eq!(stack.list().unwrap(), Vec::<String>::new());
    assert!(stack.read("materials/oak.toml").is_err());
    assert!(!stack.exists("materials/oak.toml"));
}

/// A path any pack holds exists, whether or not that pack is the one that would
/// answer a read.
#[test]
fn exists_asks_the_whole_stack() {
    let stack = Stack::mount(vec![
        pack("base", &[("materials/oak.toml", "base oak")]),
        pack("level", &[("levels/riverside.toml", "")]),
    ])
    .expect("nothing requires anything");

    assert!(stack.exists("materials/oak.toml"));
    assert!(stack.exists("levels/riverside.toml"));
    assert!(!stack.exists("materials/glass.toml"));
}

/// The stack refuses writes, and the packs under it are never offered mutably
/// at all.
///
/// `Stack` takes `Source::write`'s refusing default rather than overriding it,
/// so this is the compile-time guarantee showing up at run time: there is no
/// `&mut dyn Source` anywhere in this crate's surface for the bytes to go
/// through.
#[test]
fn nothing_can_be_written_through_a_mounted_stack() {
    let mut stack =
        Stack::mount(vec![pack("base", &[("materials/oak.toml", "base oak")])]).expect("one pack");

    assert!(stack.write("materials/oak.toml", b"edited").is_err());
    assert_eq!(stack.read("materials/oak.toml").unwrap(), *b"base oak");
}

/// The content digest sees an edit that the stamp cannot.
///
/// Two packs with the same identifier and the same version, differing in one
/// byte of one file. That is exactly the case `Stack::digest` is not asked to
/// catch and `Pack::content` is, and the pair of assertions is the statement of
/// which question each one answers.
#[test]
fn content_digests_the_bytes_and_a_stamp_does_not() {
    let shipped = pack("base", &[("materials/oak.toml", "burns = true")]);
    let edited = pack("base", &[("materials/oak.toml", "burns = false")]);

    assert_eq!(shipped.stamp(), edited.stamp());
    assert_ne!(shipped.content().unwrap(), edited.content().unwrap());
    assert_eq!(
        shipped.content().unwrap(),
        pack("base", &[("materials/oak.toml", "burns = true")])
            .content()
            .unwrap(),
        "the same bytes under the same paths digest the same",
    );
}

/// A renamed file is a different pack, because a path is what an override is
/// addressed by.
#[test]
fn content_digests_the_paths_as_well_as_the_bytes() {
    let here = pack("base", &[("materials/oak.toml", "burns = true")]);
    let there = pack("base", &[("materials/timber.toml", "burns = true")]);
    assert_ne!(here.content().unwrap(), there.content().unwrap());
}
