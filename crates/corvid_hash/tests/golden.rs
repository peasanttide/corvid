//! The frozen outputs. **Changing a value in this file is a wire-format
//! break.**
//!
//! A digest crosses the network between peers, goes into save files, and is
//! compared against traces recorded by older builds. Nothing about the
//! algorithm can change without every one of those disagreeing, and a
//! disagreement shows up as a desync or a refused save rather than as a
//! compile error. So the outputs are written down here as literals: an
//! implementation that drifts fails these tests instead of failing a player.
//!
//! If a change here is genuinely wanted, it is a new version of the format —
//! bump the crate's major version, update every golden in the workspace that
//! was recorded with the old one, and say so in the changelog. Regenerating
//! these numbers to make a red test go green is never the right move.

use core::hash::{Hash, Hasher as _};

use corvid_hash::{Digest, Hasher, digest};

/// One word absorbed, then finished.
///
/// The fifth row is the seed itself, and it is the weakest row in this file.
/// Absorbing it computes `mix(SEED ^ SEED)`, which is `mix(0)`, which is zero —
/// every stage of the mixer maps zero to zero. So the state collapses and the
/// digest is `mix(0 ^ 8)`, a function of the length alone. That still pins the
/// odd constant, the round count and the three later shift distances, but it
/// cannot see the *first* shift: the value entering the mixer is `8`, and
/// `8 >> s` is zero for every `s` above three, so changing that one distance
/// leaves this row alone while moving the other twenty-six.
///
/// It is worth keeping for exactly the reason it is weak. `mix(0) = 0` is a real
/// fixed point of this construction, and an input equal to the seed is the one
/// input a caller can reach it with; freezing what comes out is how that stays
/// deliberate rather than becoming a surprise later.
const GOLDEN_WORDS: &[(u64, u64)] = &[
    (0x0000_0000_0000_0000, 0x7383_3581_a38e_f3cd),
    (0x0000_0000_0000_0001, 0x3178_2188_0dd5_d02b),
    (0x0000_0000_0000_0002, 0xc25e_b8ea_9a0e_74e5),
    (0xffff_ffff_ffff_ffff, 0x5cc8_a00e_1392_4cb4),
    (0x9e37_79b9_7f4a_7c15, 0x982f_f8c4_33da_96d2),
    (0x8000_0000_0000_0000, 0x081b_78cf_e0c3_fd27),
    (0x0000_0000_0000_0100, 0x1e13_4d1d_68e8_be35),
    (0x0123_4567_89ab_cdef, 0x3bc4_4bc4_51dd_cc9b),
    (0x0000_0000_ffff_ffff, 0x8ddb_1346_a95e_d38d),
    (0xdead_beef_dead_beef, 0x8621_b08f_1661_ce2a),
];

/// Bytes absorbed through `write`, which counts bytes rather than words. The
/// fourth and sixth rows are the pair that proves it: the same zero-extended
/// word, three bytes and eight.
const GOLDEN_BYTES: &[(&[u8], u64)] = &[
    (&[], 0xd66f_0f54_1be4_e401),
    (&[0], 0xad32_a923_0bbf_a127),
    (&[1], 0x79be_4077_29e0_2db1),
    (&[1, 2, 3], 0x810b_0014_b0f9_33bd),
    (&[0, 0, 0, 0, 0, 0, 0, 0], 0x7383_3581_a38e_f3cd),
    (&[1, 2, 3, 0, 0, 0, 0, 0], 0xdca9_926e_b279_b821),
    (&[1, 2, 3, 4, 5, 6, 7, 8, 9], 0xf07c_2c39_9373_4c6f),
    (&[255; 32], 0xb6cf_cc2a_cdc8_6df7),
];

/// Whole values through [`digest`], which is the way almost every caller
/// reaches this crate.
///
/// The last four rows are not ASCII, and they are here because every other
/// string in this suite is. A string absorbs its *bytes*, and for ASCII the
/// byte count, the character count and the UTF-16 unit count are the same
/// number, so an ASCII-only table cannot tell an implementation that packs
/// bytes from one that packs something else. `"é"` is two bytes and one
/// character; `"🐦"` is four bytes, one character and two UTF-16 units; and the
/// last row is eleven bytes and eight characters, with the four bytes of the
/// bird straddling the eight-byte word boundary. A reimplementation in another
/// language that read the wire-format table and packed code points fails on
/// these rows and only on these rows.
const GOLDEN_VALUES: &[(&str, u64)] = &[
    ("", 0xaa46_dd0e_2501_f247),
    ("c", 0x6c55_3c7a_429d_ece4),
    ("crow", 0x54b1_bca5_42ae_e8cd),
    ("corvid", 0xb670_0a2c_8b47_cba2),
    ("the quick brown fox jumps over", 0x53fc_26c7_1849_9208),
    ("\u{e9}", 0xfd90_3b09_5182_6a67),
    ("\u{1f426}", 0x5661_d39b_01d3_a68a),
    ("na\u{ef}ve", 0x9301_cc8d_95df_62a9),
    ("corvid \u{1f426}", 0x301a_afac_ea22_1d54),
];

#[test]
fn absorbed_words_are_frozen() {
    for &(word, expected) in GOLDEN_WORDS {
        let mut hasher = Hasher::new();
        hasher.write_u64(word);
        assert_eq!(
            hasher.digest(),
            Digest::from_u64(expected),
            "word 0x{word:016x}"
        );
    }
}

#[test]
fn absorbed_bytes_are_frozen() {
    for &(bytes, expected) in GOLDEN_BYTES {
        let mut hasher = Hasher::new();
        hasher.write(bytes);
        assert_eq!(hasher.digest(), Digest::from_u64(expected), "{bytes:?}");
    }
}

#[test]
fn digested_strings_are_frozen() {
    for &(text, expected) in GOLDEN_VALUES {
        assert_eq!(digest(text), Digest::from_u64(expected), "{text:?}");
    }
}

#[test]
fn digested_structures_are_frozen() {
    assert_eq!(
        digest(&None::<u64>),
        Digest::from_u64(0x7383_3581_a38e_f3cd)
    );
    assert_eq!(digest(&Some(0u64)), Digest::from_u64(0xea5c_98e5_0dfc_de90));
    assert_eq!(
        digest(&vec![1u8, 2, 3]),
        Digest::from_u64(0x6efb_2495_62ea_12f6)
    );
    assert_eq!(
        digest(&(1u8, -1i8)),
        Digest::from_u64(0x50b9_aee9_421d_5c4d)
    );
    assert_eq!(digest(&true), Digest::from_u64(0x79be_4077_29e0_2db1));
    assert_eq!(
        digest(&'\u{1f426}'),
        Digest::from_u64(0xa25e_a7df_9dc8_c1c2)
    );
    // No row for a float, because a float has no `Hash`. What stands in its
    // place is the value a game converts one *to*: 1.5 at eight fractional
    // bits, which is an `i32` and hashes as one.
    assert_eq!(digest(&384_i32), Digest::from_u64(0x3074_2602_529d_1e5d));
}

/// Evaluated by rustc's const interpreter at compile time. The runtime path is
/// a different implementation of the same arithmetic, so the two agreeing is
/// real evidence that nothing here depends on how the host happens to compute.
const CHAINED: Digest = Hasher::new().absorb(1).absorb(2).absorb(3).digest();

#[test]
fn const_and_runtime_agree() {
    let mut runtime = Hasher::new();
    runtime.write_u64(1);
    runtime.write_u64(2);
    runtime.write_u64(3);
    assert_eq!(CHAINED, runtime.digest());
    assert_eq!(CHAINED, Digest::from_u64(0x4c1e_d98e_ebef_829f));
}

#[test]
fn a_seed_of_your_own_moves_every_output() {
    // Domain separation has to actually separate: the same input under two
    // seeds must not land anywhere near the same place.
    let mut seeded = Hasher::with_seed(0);
    seeded.write_u64(1);
    assert_ne!(seeded.digest(), Digest::from_u64(0x3178_2188_0dd5_d02b));
    assert_eq!(seeded.digest(), Digest::from_u64(0x044b_e776_f5f4_aade));
}

#[test]
fn hashing_by_hand_and_through_the_trait_agree() {
    let mut hasher = Hasher::new();
    "crow".hash(&mut hasher);
    assert_eq!(hasher.digest(), Digest::from_u64(0x54b1_bca5_42ae_e8cd));
}
