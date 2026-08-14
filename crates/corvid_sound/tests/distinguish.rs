//! That every field of every type reaches both encodings.
//!
//! The property is distinguishability: change one field of one value, and the
//! bytes and the digest must both move. A field that reaches neither is a field
//! a capture does not carry and a desync report cannot see, and neither a round
//! trip nor a frozen table would notice -- the round trip compares a value with
//! itself, and a golden row records whatever the encoder does today.
//!
//! `tests/determinism.rs` holds the sibling property -- that what does reach
//! the encoding survives coming back.

#![cfg(feature = "serde")]
#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{encode, extract};
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
fn every_field_of_every_type_reaches_the_digest() {
    // A digest that dropped a field would be a capture that stopped noticing a
    // change to it, months later and silently. One variant per field, each
    // differing from the base in exactly one place.
    let frame = populated();
    let base = digest(&frame);

    let mut moved = frame.clone();
    moved.listener = moved.listener.with_gain(Factor16::from_f64(0.1));
    assert_ne!(digest(&moved), base, "listener gain");

    let mut moved = frame.clone();
    moved.listener = Listener::new(FineTransform::IDENTITY).with_gain(frame.listener.gain);
    assert_ne!(digest(&moved), base, "listener pose");

    let mut moved = frame.clone();
    moved.buses[1].parent = None;
    assert_ne!(digest(&moved), base, "bus parent");

    let mut moved = frame.clone();
    moved.buses[1].id = BusId(2);
    assert_ne!(digest(&moved), base, "bus id");

    let mut moved = frame.clone();
    moved.buses[1].gain = Factor16::ZERO;
    assert_ne!(digest(&moved), base, "bus gain");

    let mut moved = frame.clone();
    moved.sources[0].id = SourceId(8);
    assert_ne!(digest(&moved), base, "source id");

    let mut moved = frame.clone();
    moved.sources[0].sound = SoundId(9);
    assert_ne!(digest(&moved), base, "source sound");

    let mut moved = frame.clone();
    moved.sources[0].bus = BusId::MASTER;
    assert_ne!(digest(&moved), base, "source bus");

    let mut moved = frame.clone();
    moved.sources[0].position = FinePoint::ZERO;
    assert_ne!(digest(&moved), base, "source position");

    let mut moved = frame.clone();
    moved.sources[0].gain = Factor16::ZERO;
    assert_ne!(digest(&moved), base, "source gain");

    let mut moved = frame.clone();
    moved.sources[0].pitch = I8F8::ONE;
    assert_ne!(digest(&moved), base, "source pitch");

    let mut moved = frame.clone();
    moved.sources[0].occlusion = Factor16::ZERO;
    assert_ne!(digest(&moved), base, "source occlusion");

    let mut moved = frame.clone();
    moved.cues[0].id.fired = Tick(98);
    assert_ne!(digest(&moved), base, "cue tick");

    let mut moved = frame.clone();
    moved.cues[0].id.serial = 0;
    assert_ne!(digest(&moved), base, "cue serial");

    let mut moved = frame.clone();
    moved.cues[0].sound = SoundId(9);
    assert_ne!(digest(&moved), base, "cue sound");

    let mut moved = frame.clone();
    moved.cues[0].bus = BusId::MASTER;
    assert_ne!(digest(&moved), base, "cue bus");

    let mut moved = frame.clone();
    moved.cues[0].position = FinePoint::ZERO;
    assert_ne!(digest(&moved), base, "cue position");

    let mut moved = frame.clone();
    moved.cues[0].gain = Factor16::ZERO;
    assert_ne!(digest(&moved), base, "cue gain");

    let mut moved = frame;
    moved.cues[0].pitch = I8F8::ONE;
    assert_ne!(digest(&moved), base, "cue pitch");
}

#[test]
fn every_field_of_every_type_reaches_the_bytes() {
    // The same question of the other format, because the two share nothing. A
    // `serde(skip)` on one field would leave every digest above untouched and
    // silently drop it from every capture.
    let frame = populated();
    let base = encode(&frame);

    let mut moved = frame.clone();
    moved.listener = moved.listener.with_gain(Factor16::from_f64(0.1));
    assert_ne!(encode(&moved), base, "listener gain");

    let mut moved = frame.clone();
    moved.listener = Listener::new(FineTransform::IDENTITY).with_gain(frame.listener.gain);
    assert_ne!(encode(&moved), base, "listener pose");

    let mut moved = frame.clone();
    moved.buses[1].parent = None;
    assert_ne!(encode(&moved), base, "bus parent");

    let mut moved = frame.clone();
    moved.buses[1].id = BusId(2);
    assert_ne!(encode(&moved), base, "bus id");

    let mut moved = frame.clone();
    moved.buses[1].gain = Factor16::ZERO;
    assert_ne!(encode(&moved), base, "bus gain");

    let mut moved = frame.clone();
    moved.sources[0].id = SourceId(8);
    assert_ne!(encode(&moved), base, "source id");

    let mut moved = frame.clone();
    moved.sources[0].sound = SoundId(9);
    assert_ne!(encode(&moved), base, "source sound");

    let mut moved = frame.clone();
    moved.sources[0].bus = BusId::MASTER;
    assert_ne!(encode(&moved), base, "source bus");

    let mut moved = frame.clone();
    moved.sources[0].position = FinePoint::ZERO;
    assert_ne!(encode(&moved), base, "source position");

    let mut moved = frame.clone();
    moved.sources[0].gain = Factor16::ZERO;
    assert_ne!(encode(&moved), base, "source gain");

    let mut moved = frame.clone();
    moved.sources[0].pitch = I8F8::ONE;
    assert_ne!(encode(&moved), base, "source pitch");

    let mut moved = frame.clone();
    moved.sources[0].occlusion = Factor16::ZERO;
    assert_ne!(encode(&moved), base, "source occlusion");

    let mut moved = frame.clone();
    moved.cues[0].id.fired = Tick(98);
    assert_ne!(encode(&moved), base, "cue tick");

    let mut moved = frame.clone();
    moved.cues[0].id.serial = 0;
    assert_ne!(encode(&moved), base, "cue serial");

    let mut moved = frame.clone();
    moved.cues[0].sound = SoundId(9);
    assert_ne!(encode(&moved), base, "cue sound");

    let mut moved = frame.clone();
    moved.cues[0].bus = BusId::MASTER;
    assert_ne!(encode(&moved), base, "cue bus");

    let mut moved = frame.clone();
    moved.cues[0].position = FinePoint::ZERO;
    assert_ne!(encode(&moved), base, "cue position");

    let mut moved = frame.clone();
    moved.cues[0].gain = Factor16::ZERO;
    assert_ne!(encode(&moved), base, "cue gain");

    let mut moved = frame;
    moved.cues[0].pitch = I8F8::ONE;
    assert_ne!(encode(&moved), base, "cue pitch");
}

#[test]
fn swapping_two_same_typed_fields_moves_both_encodings() {
    // `gain` and `occlusion` on a source are both `Factor16`, so nothing in the
    // type system holds their order in place. Exchanging the two *values* on a
    // source whose gain and occlusion differ has to move both encodings, or
    // neither could tell a quiet unoccluded source from a loud muffled one.
    //
    // This is the value-level half. The order the *fields* are written in is
    // frozen in `tests/golden.rs`, which is the only place that can see it.
    let source = Source::new(SourceId(0), SoundId(1))
        .with_gain(Factor16::from_f64(0.75))
        .occluded_by(Factor16::from_f64(0.125));
    let exchanged = source.with_gain(source.occlusion).occluded_by(source.gain);

    let mut a = AudioFrame::new();
    a.source(source);
    let mut b = AudioFrame::new();
    b.source(exchanged);
    assert_ne!(digest(&a), digest(&b));
    assert_ne!(encode(&a), encode(&b));
}

#[test]
fn a_root_bus_and_a_bus_parented_to_zero_do_not_collide() {
    // `Option` absorbs a discriminant word before its payload in the digest and
    // a tag byte before its payload on the wire. Without one, `None` and
    // `Some(BusId(0))` would both be nothing but zero, and the master bus would
    // be indistinguishable from a bus feeding it.
    let root = Bus::new(BusId(1));
    let parented = Bus::new(BusId(1)).under(BusId::MASTER);
    assert_ne!(root, parented);

    let mut a = AudioFrame::new();
    a.bus(root);
    let mut b = AudioFrame::new();
    b.bus(parented);
    assert_ne!(digest(&a), digest(&b));
    assert_ne!(encode(&a), encode(&b));
}

#[test]
fn extracting_the_same_thing_twice_gives_the_same_bytes() {
    // The property the whole crate is for. Two runs of the extractor over the
    // same simulation output and the same ears produce byte-identical frames,
    // so a capture recorded on one machine diffs against one recorded on
    // another.
    let bounces = [(Tick(95), 1.0), (Tick(97), 4.0), (Tick(97), -2.5)];

    let mut first = AudioFrame::new();
    extract(&mut first, &bounces, 0.25);
    let mut second = AudioFrame::new();
    extract(&mut second, &bounces, 0.25);

    assert_eq!(first, second);
    assert_eq!(digest(&first), digest(&second));
    assert_eq!(encode(&first), encode(&second));

    // And a frame that was cleared and refilled is the same as a fresh one, so
    // the reuse a runtime would depend on cannot leave a trace in the capture.
    let mut reused = first.clone();
    extract(&mut reused, &bounces, 0.25);
    assert_eq!(reused, first);
    assert_eq!(encode(&reused), encode(&first));
}
