//! Resolving an order, and the three sets of packs that have none.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

use corvid_asset::{Manifest, Pack, PackId, Stack, Unmountable};
use corvid_files::Memory;

fn id(text: &str) -> PackId {
    PackId::new(text).expect("the identifiers in these tests are short")
}

fn pack(name: &str, requires: &[&str]) -> Pack {
    let manifest = requires
        .iter()
        .fold(Manifest::new(id(name), name, 1), |manifest, needs| {
            manifest.requiring(id(needs))
        });
    Pack::new(manifest, Memory::new())
}

fn mounted(stack: &Stack) -> Vec<PackId> {
    stack
        .packs()
        .iter()
        .map(|pack| pack.manifest().id)
        .collect()
}

/// A requirement nothing in the set answers to refuses the mount and says which
/// pack wanted what.
///
/// Mounting anyway would give the level a stack with nothing under it: every
/// path it meant to override would be a path it defines, and the failure would
/// surface as missing content somewhere with no manifest in sight.
#[test]
fn a_requirement_nothing_answers_to_refuses_the_mount() {
    let refused = Stack::mount(vec![pack("riverside", &["terminus"])])
        .expect_err("terminus is not in the set");

    assert_eq!(
        refused,
        Unmountable::Absent {
            by: id("riverside"),
            needs: id("terminus"),
        },
    );
    assert_eq!(
        refused.to_string(),
        "riverside requires terminus, which is not in the stack",
    );
}

/// Requirements that lead back to where they started refuse rather than loop,
/// and the message names every pack that could not be placed.
#[test]
fn a_cycle_refuses_rather_than_looping() {
    let refused = Stack::mount(vec![
        pack("weather", &["seasons"]),
        pack("seasons", &["weather"]),
    ])
    .expect_err("neither can go first");

    assert_eq!(
        refused,
        Unmountable::Cycle {
            packs: vec![id("weather"), id("seasons")],
        },
    );
    assert_eq!(
        refused.to_string(),
        "these packs require each other and cannot be ordered: weather seasons",
    );
}

/// A pack that requires itself is a cycle of one and is caught by the same
/// walk, rather than being a special case somebody had to think of.
#[test]
fn a_pack_that_requires_itself_is_a_cycle() {
    let refused =
        Stack::mount(vec![pack("weather", &["weather"])]).expect_err("it can never go first");
    assert_eq!(
        refused,
        Unmountable::Cycle {
            packs: vec![id("weather")],
        },
    );
}

/// The pack a cycle drags down is named too.
///
/// `terminus` is fine on its own; it is only unplaceable because it requires a
/// pack inside the loop. Reporting the loop alone would leave a person to work
/// out for themselves why their base did not mount.
#[test]
fn a_cycle_names_everything_it_stranded() {
    let refused = Stack::mount(vec![
        pack("terminus", &["weather"]),
        pack("weather", &["seasons"]),
        pack("seasons", &["weather"]),
    ])
    .expect_err("nothing can go first");

    assert_eq!(
        refused,
        Unmountable::Cycle {
            packs: vec![id("terminus"), id("weather"), id("seasons")],
        },
    );
}

/// One identifier claimed twice refuses, because every `requires` in every
/// other pack would otherwise name two packs at once.
#[test]
fn one_identifier_claimed_twice_refuses_the_mount() {
    let refused = Stack::mount(vec![pack("terminus", &[]), pack("terminus", &[])])
        .expect_err("both call themselves terminus");

    assert_eq!(refused, Unmountable::Twice { id: id("terminus") });
    assert_eq!(refused.to_string(), "terminus is in the stack twice");
}

/// Packs that need nothing keep the order they were offered in.
///
/// This is what makes mount order a thing a session can state. Sorting the ties
/// by identifier would have put `base` after `alpha` here and made every load
/// order alphabetical.
#[test]
fn independent_packs_keep_the_order_they_were_offered_in() {
    let stack = Stack::mount(vec![
        pack("zinc", &[]),
        pack("base", &[]),
        pack("alpha", &[]),
    ])
    .expect("nothing requires anything");

    assert_eq!(mounted(&stack), [id("zinc"), id("base"), id("alpha")]);
}

/// A pack lands above what it requires however the two were offered.
///
/// `requires` already says which way round they go, so a caller who listed the
/// level first gets a working stack rather than an error telling them to write
/// the same fact twice.
#[test]
fn a_pack_lands_above_what_it_requires() {
    let offered_the_wrong_way = Stack::mount(vec![
        pack("riverside", &["terminus"]),
        pack("terminus", &[]),
    ])
    .expect("terminus is in the set");
    let offered_the_right_way = Stack::mount(vec![
        pack("terminus", &[]),
        pack("riverside", &["terminus"]),
    ])
    .expect("terminus is in the set");

    assert_eq!(
        mounted(&offered_the_wrong_way),
        [id("terminus"), id("riverside")],
    );
    assert_eq!(
        mounted(&offered_the_right_way),
        mounted(&offered_the_wrong_way),
    );
}

/// A chain of requirements comes out bottom-first, and the packs beside it stay
/// where they were put.
#[test]
fn a_chain_of_requirements_resolves_and_leaves_the_rest_alone() {
    let stack = Stack::mount(vec![
        pack("music", &[]),
        pack("riverside", &["weather"]),
        pack("weather", &["terminus"]),
        pack("terminus", &[]),
    ])
    .expect("every requirement is in the set");

    assert_eq!(
        mounted(&stack),
        [id("music"), id("terminus"), id("weather"), id("riverside"),],
        "music needed nothing and was offered first, so it stays first",
    );
}
