//! That a frame survives being written down, and comes back the same.
//!
//! # What a round trip is not
//!
//! Serializing a value and deserializing it again says nothing about the
//! *format*. The writer and the reader are derived from one declaration and
//! move together, so exchanging two fields, widening an identifier or adding a
//! field changes every byte a capture holds and leaves every assertion in this
//! file green. A digest golden does not cover it either: the digest encoding is
//! hand-written and independent of the derived one, so a field reordering moves
//! the bytes and no digest at all. The bytes themselves have to be written
//! down, and `tests/golden.rs` is where they are.
//!
//! What a round trip *does* cover is that nothing is lost on the way through,
//! which is the other half and is not implied by a frozen table: a field the
//! encoder drops has a stable golden row and comes back wrong.
//!
//! `tests/distinguish.rs` holds the sibling property -- that every field
//! reaches the encoding at all.

#![cfg(feature = "serde")]
#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{decode, encode};
use corvid_fixed::{Factor16, I8F8, I16F16, I48F16};
use corvid_hash::digest;
use corvid_sound::{AudioFrame, Bus, BusId, Cue, CueId, Listener, SoundId, Source, SourceId};
use corvid_time::Tick;
use corvid_transform::FineTransform;
use corvid_vector::{FinePoint, GlobalFinePoint};

/// A frame with something in every list and no two fields sharing a value, so
/// that a field emitted in the wrong place has somewhere different to land.
fn populated() -> AudioFrame {
    let mut frame = AudioFrame::new();
    frame.listen(
        Listener::new(FineTransform::IDENTITY.with_position(GlobalFinePoint::new(
            I48F16::from_f64(1.0),
            I48F16::from_f64(2.0),
            I48F16::from_f64(3.0),
        )))
        .with_gain(Factor16::from_f64(0.875)),
    );
    frame.bus(Bus::new(BusId::MASTER).with_gain(Factor16::from_f64(0.5)));
    frame.bus(
        Bus::new(BusId(1))
            .under(BusId::MASTER)
            .with_gain(Factor16::from_f64(0.25)),
    );
    frame.source(
        Source::new(SourceId(7), SoundId(2))
            .on(BusId(1))
            .at(FinePoint::new(
                I16F16::from_f64(4.0),
                I16F16::from_f64(5.0),
                I16F16::from_f64(6.0),
            ))
            .with_gain(Factor16::from_f64(0.75))
            .with_pitch(I8F8::from_f64(1.5))
            .occluded_by(Factor16::from_f64(0.125)),
    );
    frame.cue(
        Cue::new(CueId::new(Tick(97), 1), SoundId(3))
            .on(BusId(1))
            .at(FinePoint::new(
                I16F16::from_f64(-7.0),
                I16F16::from_f64(8.0),
                I16F16::from_f64(-9.0),
            ))
            .with_gain(Factor16::from_f64(0.625))
            .with_pitch(I8F8::from_f64(0.5)),
    );
    frame
}

#[test]
fn a_frame_round_trips_by_value() {
    let frame = populated();
    let bytes = encode(&frame);
    let read: AudioFrame = decode(&bytes);
    assert_eq!(read, frame);

    // Every byte written was read, and a decoder that stopped early would have
    // failed rather than looking like a successful read of a shorter format --
    // `corvid_wire::decode` refuses leftovers, so this is the case that says so
    // for a frame rather than for a pair of integers.
    let mut grown = bytes;
    grown.push(0);
    assert!(corvid_wire::decode::<AudioFrame>(&grown).is_err());
}

#[test]
fn a_frame_round_trips_by_digest() {
    // By value and by digest are two different questions, because `PartialEq`
    // and `Hash` are two independent implementations and a frame could satisfy
    // one without the other. A capture is compared by digest, so this is the one
    // that matches how the format is actually used.
    let frame = populated();
    let read: AudioFrame = decode(&encode(&frame));
    assert_eq!(digest(&read), digest(&frame));
}

#[test]
fn an_empty_frame_round_trips_too() {
    let frame = AudioFrame::new();
    let read: AudioFrame = decode(&encode(&frame));
    assert_eq!(read, frame);
    assert!(read.is_empty());
}

#[test]
fn the_lists_are_length_prefixed_in_the_digest() {
    let source = Source::new(SourceId(1), SoundId(2))
        .on(BusId(3))
        .at(FinePoint::new(
            I16F16::from_bits(4),
            I16F16::from_bits(5),
            I16F16::from_bits(6),
        ))
        .with_gain(Factor16::from_bits(7))
        .with_pitch(I8F8::from_bits(8))
        .occluded_by(Factor16::from_bits(9));
    let cue = Cue::new(CueId::new(Tick(1), 2), SoundId(3))
        .on(BusId(4))
        .at(FinePoint::new(
            I16F16::from_bits(5),
            I16F16::from_bits(6),
            I16F16::from_bits(7),
        ))
        .with_gain(Factor16::from_bits(8))
        .with_pitch(I8F8::from_bits(9));

    // A sound that keeps playing and a bang that happened once are two
    // different frames, and they land in two different lists.
    let mut sourced = AudioFrame::new();
    sourced.source(source);
    let mut cued = AudioFrame::new();
    cued.cue(cue);
    assert_ne!(digest(&sourced), digest(&cued));

    // The claim this test is named for: the same element twice is not the same
    // element once, because each list absorbs its length before its contents.
    // Without the prefix these two absorb one run of words and a longer run of
    // the same words, and only the injected byte count would be between them.
    let mut twice = AudioFrame::new();
    twice.source(source);
    twice.source(source);
    assert_ne!(digest(&sourced), digest(&twice));

    // And an empty list is not an absent one: a frame carrying both is neither
    // of the two frames that carry one.
    let mut both = AudioFrame::new();
    both.source(source);
    both.cue(cue);
    assert_ne!(digest(&both), digest(&sourced));
    assert_ne!(digest(&both), digest(&cued));
}
