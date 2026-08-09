//! What the frame distinguishes, and that it survives being written down.
//!
//! # What a round trip is not
//!
//! Serializing a value and deserializing it again says nothing about the
//! *format*. The writer and the reader are derived from one declaration and
//! move together, so exchanging two fields, widening an identifier or adding a
//! field changes every byte a capture holds and leaves every assertion in this
//! file green. A digest golden does not cover it either: the digest encoding is
//! hand-written and independent of the derived one, so a field reordering moves
//! the bytes and no digest at all. The bytes themselves have to be written down,
//! and `tests/golden.rs` is where they are.
//!
//! What a round trip does say is that the two derived halves are inverses over
//! the values this crate actually builds — that nothing is skipped, nothing is
//! narrowed on the way out, and `PartialEq` agrees with what came back. That is
//! worth having and it is all that is claimed here.
//!
//! The rest of this file is the other question: whether the encodings *separate*
//! the things a frame has to keep apart. Those are comparisons between two
//! outputs, so they catch an encoding that stopped distinguishing two frames and
//! cannot catch one that distinguishes them differently than it did when the
//! table was recorded.

// A frame has to survive being written down, so the claims here need `serde`.
#![cfg(feature = "serde")]
#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{decode, encode, extract};
use corvid_fixed::{Factor16, I8F8, I16F16, I48F16};
use corvid_hash::digest;
use corvid_sound::{AudioFrame, Bus, BusId, Cue, CueId, Listener, SoundId, Source, SourceId};
use corvid_time::Tick;
use corvid_transform::GlobalFineTransform;
use corvid_vector::{FinePoint, GlobalFinePoint};

/// A frame with something in every list and no two fields sharing a value, so
/// that a field emitted in the wrong place has somewhere different to land.
fn populated() -> AudioFrame {
    let mut frame = AudioFrame::new();
    frame.listen(
        Listener::new(
            GlobalFineTransform::IDENTITY.with_position(GlobalFinePoint::new(
                I48F16::from_f64(1.0),
                I48F16::from_f64(2.0),
                I48F16::from_f64(3.0),
            )),
        )
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
    // failed rather than looking like a successful read of a shorter format —
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
    moved.listener = Listener::new(GlobalFineTransform::IDENTITY).with_gain(frame.listener.gain);
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
    moved.listener = Listener::new(GlobalFineTransform::IDENTITY).with_gain(frame.listener.gain);
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
