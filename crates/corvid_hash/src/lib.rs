#![doc = include_str!("../README.md")]
#![no_std]
#![allow(
    clippy::redundant_pub_crate,
    reason = "these modules are private, so pub(crate) and pub are equivalent — pub(crate) is the one that says what is meant, and keeps the helpers from looking like API if a module is ever made public"
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
//
// What the refusal closes is the byte-order half of that hole and only that
// half. The same specialisation covers `usize` and `isize`, and what it hands
// to `write` there is `size_of_val`'s bytes — four per element on `wasm32` and
// eight on a native server — so a `Vec<usize>` in hashed state still parts two
// peers that agree about endianness perfectly well. Refusing to build is not
// available as an answer to that one, because a 32-bit target is precisely the
// target this crate exists to keep in agreement. What closes it is hashed state
// naming a fixed-width integer type, which is a discipline rather than a
// compiler error; the `Hasher` type's documentation says so and
// `tests/width.rs` pins the behaviour that makes it necessary.
#[cfg(target_endian = "big")]
compile_error!(
    "corvid_hash produces a canonical digest only on little-endian targets: \
     `Hash::hash_slice` absorbs a slice of primitives in the target's own byte \
     order and cannot be overridden"
);

mod hasher;
mod mix;

pub use hasher::{Digest, Hasher, digest, digest_with_seed};
