#![doc = include_str!("../README.md")]
#![no_std]
#![allow(
    clippy::redundant_pub_crate,
    reason = "these modules are private, so pub(crate) and pub are equivalent — pub(crate) is the one that says what is meant, and keeps the helpers from looking like API if a module is ever made public"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "`usize` and `isize` are absorbed as 64 bits whatever the target's pointer width is, which is the crate's whole reason for overriding those two writes; no target has a pointer wider than 64 bits, so nothing is lost"
)]

// A digest is only canonical where the target is little-endian, and this is
// where that stops being a hope.
//
// `Hasher` overrides every `write_*` so an integer absorbs little-endian at its
// declared width. What it cannot override is `Hash::hash_slice`, which `core`
// implements for every primitive integer by reinterpreting the whole slice as
// bytes and calling `write` once — so a `Vec<u32>` in a hashed state absorbs the
// target's own byte order, past every override, with nothing at the call site to
// see. Two peers of opposite endianness would compute identical states and
// exchange different marks on the first tick.
//
// There is no fix inside the hasher: `write` is handed bytes and is not told
// what they were. So a target that would be wrong does not build. Every target
// this workspace names is little-endian, and a big-endian one is a decision to
// take deliberately rather than to discover from a desync.
#[cfg(target_endian = "big")]
compile_error!(
    "corvid_hash produces a canonical digest only on little-endian targets: \
     `Hash::hash_slice` absorbs a slice of primitives in the target's own byte \
     order and cannot be overridden"
);

mod hasher;
mod mix;

pub use hasher::{Digest, Hasher, digest, digest_with_seed};
