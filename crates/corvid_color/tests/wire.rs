//! What a colour looks like once it has left the process.

// A build without `serde` compiles nothing here rather than half of it.
#![cfg(feature = "serde")]
#![allow(
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::panic_in_result_fn,
    reason = "tests assert; panicking is the failure mode"
)]

use corvid_color::{LinearRgba, Oklab, Oklch, Rgba8};
use corvid_fixed::{Angle32, I2F30, I16F16};
use corvid_hash::digest;

/// The same colour digests the same, and a neighbouring one does not.
#[test]
fn a_colour_digests() {
    let ember = Rgba8::opaque_hex(0xE5_78_29);
    assert_eq!(digest(&ember), digest(&Rgba8::opaque_hex(0xE5_78_29)));
    assert_ne!(digest(&ember), digest(&Rgba8::opaque_hex(0xE5_78_2A)));
}

/// Coverage is in the digest, so two colours that differ only in it do not
/// collide.
#[test]
fn coverage_is_hashed() {
    assert_ne!(
        digest(&Rgba8::rgb(1, 2, 3)),
        digest(&Rgba8::new(1, 2, 3, 254))
    );
}

/// The channels are absorbed in declaration order rather than as an unordered
/// set, so a colour and its own permutation differ.
#[test]
fn the_channels_are_ordered() {
    assert_ne!(digest(&Rgba8::rgb(1, 2, 3)), digest(&Rgba8::rgb(3, 2, 1)));
}

/// Every public type survives the workspace's encoding unchanged.
#[test]
fn every_type_round_trips_on_the_wire() -> Result<(), corvid_wire::Error> {
    let ember = Rgba8::opaque_hex(0xE5_78_29);
    assert_eq!(
        corvid_wire::decode::<Rgba8>(&corvid_wire::encode(&ember)?)?,
        ember
    );

    let linear = ember.to_linear();
    assert_eq!(
        corvid_wire::decode::<LinearRgba>(&corvid_wire::encode(&linear)?)?,
        linear
    );

    let lab = Oklab::from_linear(linear);
    assert_eq!(
        corvid_wire::decode::<Oklab>(&corvid_wire::encode(&lab)?)?,
        lab
    );

    let lch = Oklch::new(
        I2F30::from_f64(0.7),
        I2F30::from_f64(0.15),
        Angle32::from_turns(0.25),
        I16F16::ONE,
    );
    assert_eq!(
        corvid_wire::decode::<Oklch>(&corvid_wire::encode(&lch)?)?,
        lch
    );

    Ok(())
}

/// Every type in the crate digests, not only the byte one.
///
/// **This is what fixed point buys.** `f32` and `f64` have no `Hash`, so a
/// float-bearing colour could not reach a golden, a UI layout digest or a
/// capture at all.
#[test]
fn every_type_digests() {
    let ember = Rgba8::opaque_hex(0xE5_78_29);
    let linear = ember.to_linear();
    let lab = Oklab::from_linear(linear);

    assert_eq!(digest(&linear), digest(&ember.to_linear()));
    assert_ne!(
        digest(&linear),
        digest(&Rgba8::opaque_hex(0xE5_78_2A).to_linear())
    );

    assert_eq!(digest(&lab), digest(&Oklab::from_linear(linear)));
    assert_ne!(
        digest(&lab),
        digest(&Oklab::from_linear(
            Rgba8::opaque_hex(0x29_78_E5).to_linear()
        ))
    );

    let lch = lab.to_oklch();
    assert_eq!(digest(&lch), digest(&Oklab::from_linear(linear).to_oklch()));
    assert_ne!(
        digest(&lch),
        digest(&Oklch::new(
            lch.l,
            lch.c,
            lch.h.wrapping_add(Angle32::from_turns(0.1)),
            lch.alpha
        ))
    );
}

/// A colour is four bytes on the wire and nothing else — no length prefix, no
/// field names, no padding. The recorded string is what freezes that: a change
/// to the encoding shows up here rather than in a golden nobody can read.
#[test]
fn a_colour_is_four_bytes() -> Result<(), corvid_wire::Error> {
    let bytes = corvid_wire::encode(&Rgba8::new(1, 2, 3, 4))?;
    assert_eq!(bytes, [1, 2, 3, 4]);
    Ok(())
}
