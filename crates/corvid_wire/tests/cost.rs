//! What a varint costs at scale, measured rather than remembered.
//!
//! The README's "What it costs" table quotes three sizes, and a size in a README
//! is a number nobody rechecks. These are the same three, computed here, so an
//! upgrade that changed how a marker is spelled moves a test rather than leaving
//! the prose quietly wrong.
//!
//! Fifty thousand of each: the scale a rollback in this workspace is budgeted
//! for, and large enough that the count in front of the list -- three bytes, the
//! marker `fb` and two more -- is a rounding error against the elements.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_wire::encode;

/// The scale the README quotes.
const MANY: u32 = 50_000;

/// The count in front of a list of that many: a marker and two bytes.
const PREFIX: usize = 3;

#[test]
fn a_count_that_climbs_from_zero_is_cheaper_than_its_declared_width() {
    let ids: Vec<u32> = (0..MANY).collect();

    // Under 251 for the first 251 of them, two bytes to 65,535, three above it --
    // so the column averages three bytes where a declared `u32` is always four.
    assert_eq!(encode(&ids).unwrap().len(), 149_501);
    assert!(149_501 < PREFIX + 4 * MANY as usize);
}

#[test]
fn a_field_that_uses_its_bits_is_dearer_than_its_declared_width() {
    // Every value above 65,535, so every one of them is a marker and four bytes.
    let saturated: Vec<u32> = (0..MANY).map(|id| 0x8000_0000 | id).collect();

    assert_eq!(encode(&saturated).unwrap().len(), 250_003);
    assert_eq!(250_003, PREFIX + 5 * MANY as usize);
}

#[test]
fn a_digest_is_the_worst_case_and_a_trace_is_nothing_else() {
    // A digest uses all sixty-four of its bits by construction, so it is always
    // the marker and eight. A trace is one of these per tick and nothing else,
    // which makes it the one thing in the workspace this encoding makes larger.
    let digests: Vec<u64> = (0..u64::from(MANY))
        .map(|tick| 0x8000_0000_0000_0000 | tick)
        .collect();

    assert_eq!(encode(&digests).unwrap().len(), 450_003);
    assert_eq!(450_003, PREFIX + 9 * MANY as usize);
}
