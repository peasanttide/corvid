//! The values the three golden tables are recorded over.
//!
//! Shared because the tables have to cover the *same* values to be read
//! together: a row in one and not another is a gap that reads as agreement.
//! They live here rather than in whichever file was largest, so that none of
//! the three owns the fixtures the other two borrow.

#![allow(
    dead_code,
    reason = "each golden binary uses the subset of these its own table covers"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "this module is private to each test binary, so pub(crate) and pub are equivalent -- pub(crate) is the one rustc's unreachable_pub asks for, and the two lints cannot both be satisfied"
)]

use corvid_fixed::{Factor16, I8F8, I16F16, I48F16};
use corvid_sound::{AudioFrame, Bus, BusId, Cue, CueId, Listener, SoundId, Source, SourceId};
use corvid_time::Tick;
use corvid_transform::FineTransform;
use corvid_vector::{FinePoint, GlobalFinePoint};

// ---------------------------------------------------------------------------
// The fixtures, shared by both halves.
// ---------------------------------------------------------------------------

/// Every identifier the tables below cover, including the widest value each
/// type can hold.
///
/// The saturated rows are not decoration. A varint spells a small number the
/// same however it was declared, so a row holding 0 or 1 says nothing about an
/// identifier's width at all -- the saturated row is where the marker and the
/// four bytes of `ff` appear, and it is the only byte row a widening or a
/// narrowing runs into.
pub(crate) const SOUND_IDS: &[SoundId] = &[SoundId(0), SoundId(1), SoundId(2), SoundId(u32::MAX)];
pub(crate) const BUS_IDS: &[BusId] = &[BusId::MASTER, BusId(1), BusId(u16::MAX)];
pub(crate) const SOURCE_IDS: &[SourceId] = &[SourceId(7), SourceId(u32::MAX)];

/// The cue identities, whose two halves have different widths.
pub(crate) const CUE_IDS: &[CueId] = &[
    CueId::new(Tick(97), 0),
    CueId::new(Tick(97), 1),
    CueId::new(Tick(98), 0),
    CueId::new(Tick(u64::MAX), u16::MAX),
];

/// A source with a different value in every field.
pub(crate) const fn full_source() -> Source {
    Source::new(SourceId(7), SoundId(2))
        .on(BusId(1))
        .at(FinePoint::new(
            I16F16::from_f64(4.0),
            I16F16::from_f64(5.0),
            I16F16::from_f64(6.0),
        ))
        .with_gain(Factor16::from_f64(0.75))
        .with_pitch(I8F8::from_f64(1.5))
        .occluded_by(Factor16::from_f64(0.125))
}

/// A cue with a different value in every field.
pub(crate) const fn full_cue() -> Cue {
    Cue::new(CueId::new(Tick(97), 1), SoundId(3))
        .on(BusId(1))
        .at(FinePoint::new(
            I16F16::from_f64(-7.0),
            I16F16::from_f64(8.0),
            I16F16::from_f64(-9.0),
        ))
        .with_gain(Factor16::from_f64(0.625))
        .with_pitch(I8F8::from_f64(0.5))
}

pub(crate) const fn full_listener() -> Listener {
    Listener::new(FineTransform::IDENTITY.with_position(GlobalFinePoint::new(
        I48F16::from_f64(1.0),
        I48F16::from_f64(2.0),
        I48F16::from_f64(3.0),
    )))
    .with_gain(Factor16::from_f64(0.875))
}

/// The buses, whose three fields are emitted in declaration order.
///
/// The first two rows are the `Option` pair: a root bus and a bus parented to
/// bus zero. They differ in one byte of the serialized form and one word of the
/// digest, and that byte and that word are the whole of what keeps the master
/// bus distinguishable from a bus feeding it.
pub(crate) fn every_bus() -> Vec<Bus> {
    vec![
        Bus::new(BusId(1)),
        Bus::new(BusId(1)).under(BusId::MASTER),
        Bus::new(BusId(1))
            .under(BusId::MASTER)
            .with_gain(Factor16::from_f64(0.5)),
        Bus::default(),
    ]
}

/// The sources, whose seven fields are emitted in declaration order.
///
/// Every field of the second row holds a value no other field of that row
/// holds, which is what makes the order visible: a source that emitted its
/// fields backwards would still encode to something, and would still tell two
/// different sources apart, and would still pass every relative test here. The
/// third row is that source with `gain` and `occlusion` exchanged.
pub(crate) fn every_source() -> Vec<Source> {
    let full = full_source();
    vec![
        Source::new(SourceId(0), SoundId(1)),
        full,
        full.with_gain(full.occlusion).occluded_by(full.gain),
    ]
}

/// The cues, whose identity is emitted before their payload.
///
/// The last three rows are the three-way distinction the whole crate turns on,
/// written out so that each half of it is frozen. Rows two and three are the
/// same payload under two identities -- a second bounce on the same tick -- and
/// rows two and four are the same identity under two payloads -- one bounce
/// heard from two places as the listener walked. All three differ, which is what
/// makes an encoding a change detector and not an identity, and an identity not
/// a change detector. Both are in the frame because neither can do the other's
/// job.
pub(crate) fn every_cue() -> Vec<Cue> {
    let full = full_cue();
    vec![
        Cue::new(CueId::first(Tick(97)), SoundId(1)),
        full,
        Cue {
            id: CueId::new(Tick(97), 2),
            ..full
        },
        full.at(full
            .position
            .sub(FinePoint::new(I16F16::ONE, I16F16::ZERO, I16F16::ZERO))),
    ]
}

pub(crate) fn every_listener() -> Vec<Listener> {
    vec![Listener::default(), full_listener()]
}

/// The frames.
///
/// The empty frame is the row that pins the three list lengths: without them, a
/// frame with nothing in it would be its listener and stop, and a later format
/// that added a fourth list would collide with it.
pub(crate) fn every_frame() -> Vec<AudioFrame> {
    let mut one_source = AudioFrame::new();
    one_source.source(Source::new(SourceId(0), SoundId(1)));

    let mut one_cue = AudioFrame::new();
    one_cue.cue(Cue::new(CueId::first(Tick(97)), SoundId(1)));

    vec![AudioFrame::new(), one_source, one_cue, populated()]
}

/// One of everything, which is the shape a captured frame actually has.
pub(crate) fn populated() -> AudioFrame {
    let mut frame = AudioFrame::new();
    frame.listen(full_listener());
    frame.bus(Bus::new(BusId::MASTER).with_gain(Factor16::from_f64(0.5)));
    frame.bus(
        Bus::new(BusId(1))
            .under(BusId::MASTER)
            .with_gain(Factor16::from_f64(0.25)),
    );
    frame.source(full_source());
    frame.cue(full_cue());
    frame
}
