//! A stand-in for a game's `hear`, so the cue tests have something that
//! behaves like an extractor rather than a hand-written list of cues.
//!
//! It is deliberately the smallest thing that shows the shape: the simulation
//! says which ticks a thing bounced on and where, the client says where the
//! ears are, and the extractor turns the two into a frame. Everything the
//! rollback tests need is a consequence of that split — the bounces come from
//! the simulation and survive a rollback only if the re-simulation produces
//! them again, and the ears come from the client and move between two
//! extractions of the same tick.

#![allow(
    dead_code,
    reason = "each integration test binary compiles this module separately, so anything only one of them uses is dead in the others"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "this module is private to each test binary, so pub(crate) and pub are equivalent — pub(crate) is the one rustc's unreachable_pub asks for, and the two lints cannot both be satisfied"
)]
#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_fixed::{Factor16, I16F16};

use corvid_sound::{AudioFrame, Cue, Listener, SoundId};

use corvid_time::Tick;

use corvid_vector::FinePoint;
/// The one sound these tests use.
pub(crate) const THUD: SoundId = SoundId(1);

/// What the simulation says happened: a tick, and where in the world.
///
/// A pair rather than a struct because these tests only ever write them as
/// literals, and the second field is a world x-coordinate in metres.
pub(crate) type Bounce = (Tick, f64);

/// Extracts `bounces` into `frame` with the ears at world x `ears`.
///
/// The order of `bounces` is the order the cues are emitted in, and so is the
/// order the serials are assigned in. That is the obligation the crate
/// documentation puts on an extractor, made visible: pass the same slice twice
/// and the identities match, shuffle it and they do not.
pub(crate) fn extract(frame: &mut AudioFrame, bounces: &[Bounce], ears: f64) {
    frame.clear();
    frame.listen(Listener::default());
    for &(fired, x) in bounces {
        let id = frame.next_id(fired);
        // The position is an offset in the listener's frame, so moving the ears
        // moves every cue in the frame. This is the whole reason a payload
        // cannot be an identity.
        let offset = FinePoint::new(I16F16::from_f64(x - ears), I16F16::ZERO, I16F16::ZERO);
        // And the gain falls off with that offset, so a listener that moved has
        // changed two payload fields rather than one.
        let gain = Factor16::from_f64(1.0 / (1.0 + (x - ears).abs()));
        frame.cue(Cue::new(id, THUD).at(offset).with_gain(gain));
    }
}

/// Encodes `value` the way a capture is written.
///
/// One line over [`corvid_wire::encode`], and it is here so that no test in
/// this crate has to name a format: the encoding a recorded byte string is
/// recorded under is a workspace-wide decision, and a test file that could
/// choose its own would be free to choose a different one.
#[cfg(feature = "serde")]
pub(crate) fn encode<T: serde::Serialize + ?Sized>(value: &T) -> Vec<u8> {
    corvid_wire::encode(value).unwrap()
}

/// Reads one back, refusing bytes with anything left over.
///
/// The leftovers are refused by [`corvid_wire::decode`] itself rather than
/// counted here, which is the point: a decoder that stopped early would look
/// like a successful read of a shorter format, and no caller has to remember to
/// check for it.
#[cfg(feature = "serde")]
pub(crate) fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> T {
    corvid_wire::decode(bytes).unwrap()
}
