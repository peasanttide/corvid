//! Building a frame, emptying it, and refilling it without allocating.

use corvid_fixed::Factor16;

use corvid_sound::{AudioFrame, Bus, BusId, Cue, CueId, Listener, SoundId, Source, SourceId};

use corvid_time::Tick;

use corvid_transform::FineTransform;
const THUD: SoundId = SoundId(1);
const TORCH: SoundId = SoundId(2);

/// Fills `frame` with two of each kind of entry and a listener that is not the
/// default one.
fn fill(frame: &mut AudioFrame) {
    frame.listen(Listener::default().with_gain(Factor16::from_f64(0.25)));
    frame.source(Source::new(SourceId(0), TORCH));
    frame.source(Source::new(SourceId(1), TORCH));
    let first = frame.next_id(Tick(1));
    frame.cue(Cue::new(first, THUD));
    let next = frame.next_id(Tick(1));
    frame.cue(Cue::new(next, THUD));
    frame.bus(Bus::new(BusId::MASTER));
    frame.bus(Bus::new(BusId(1)).under(BusId::MASTER));
}

#[test]
fn a_new_frame_is_empty_and_hears_everything() {
    let frame = AudioFrame::new();
    assert!(frame.is_empty());
    assert_eq!(frame.listener.pose, FineTransform::IDENTITY);

    // The listener's default gain is written by hand rather than derived,
    // because a derived `Factor16` is zero and a frame nobody gave a listener
    // to would be silent instead of centred.
    assert_eq!(frame.listener.gain, Factor16::ONE);
    assert_eq!(Listener::default().gain, Factor16::ONE);
    assert_eq!(AudioFrame::default().listener.gain, Factor16::ONE);
    assert_eq!(AudioFrame::default(), AudioFrame::new());
}

#[test]
fn a_frame_with_only_buses_is_still_empty() {
    // `is_empty` answers "is there anything to hear", and a routing table is
    // not a sound. A listener is not one either.
    let mut frame = AudioFrame::new();
    frame.bus(Bus::new(BusId::MASTER));
    frame.listen(Listener::default().with_gain(Factor16::ZERO));
    assert!(frame.is_empty());

    frame.source(Source::new(SourceId(0), TORCH));
    assert!(!frame.is_empty());

    let mut cued = AudioFrame::new();
    cued.cue(Cue::new(CueId::first(Tick(0)), THUD));
    assert!(!cued.is_empty());
}

#[test]
fn clearing_resets_the_listener_as_well_as_the_lists() {
    let mut frame = AudioFrame::new();
    fill(&mut frame);
    assert_ne!(frame.listener, Listener::default());

    frame.clear();

    // A `clear` that emptied the three lists and left the listener behind would
    // hand the next extractor last frame's ears, which on the tick a game
    // switched cameras is a frame mixed from the wrong place.
    assert_eq!(frame.listener, Listener::default());
    assert!(frame.sources.is_empty());
    assert!(frame.cues.is_empty());
    assert!(frame.buses.is_empty());
    assert!(frame.is_empty());
    assert_eq!(frame, AudioFrame::new());
}

#[test]
fn clearing_and_refilling_does_not_allocate_again() {
    let mut frame = AudioFrame::new();
    fill(&mut frame);

    let (sources, cues, buses) = (
        frame.sources.capacity(),
        frame.cues.capacity(),
        frame.buses.capacity(),
    );
    assert!(
        sources > 0 && cues > 0 && buses > 0,
        "nothing was filled in"
    );

    // A runtime is meant to hold one frame for the life of the process and hand
    // it to the extractor once per displayed frame, so this loop is the steady
    // state rather than a corner case. Assigning a fresh `Vec` in `clear` would
    // drop every one of these to zero.
    //
    // This is evidence about these three vectors and not a proof that a
    // clear-and-refill allocates nothing: capacity is what a reallocation would
    // move, and it is all this can see.
    for _ in 0..8 {
        frame.clear();
        assert_eq!(frame.sources.capacity(), sources, "sources reallocated");
        assert_eq!(frame.cues.capacity(), cues, "cues reallocated");
        assert_eq!(frame.buses.capacity(), buses, "buses reallocated");
        fill(&mut frame);
        assert_eq!(frame.sources.capacity(), sources, "sources grew on refill");
        assert_eq!(frame.cues.capacity(), cues, "cues grew on refill");
        assert_eq!(frame.buses.capacity(), buses, "buses grew on refill");
    }
}

#[test]
fn next_id_numbers_within_a_tick_and_starts_over_for_another() {
    let mut frame = AudioFrame::new();

    let first = frame.next_id(Tick(97));
    assert_eq!(first, CueId::new(Tick(97), 0));
    assert_eq!(first, CueId::first(Tick(97)));
    frame.cue(Cue::new(first, THUD));

    let second = frame.next_id(Tick(97));
    assert_eq!(second, CueId::new(Tick(97), 1));
    frame.cue(Cue::new(second, THUD));

    // A different tick is numbered from zero regardless of how many cues are
    // already in the frame, because a serial is a position within its tick and
    // not within the frame.
    let other = frame.next_id(Tick(98));
    assert_eq!(other, CueId::new(Tick(98), 0));
    frame.cue(Cue::new(other, THUD));

    // And going back to the first tick continues where it left off, so an
    // extractor that emits two ticks interleaved still numbers each correctly.
    assert_eq!(frame.next_id(Tick(97)), CueId::new(Tick(97), 2));
    assert_eq!(frame.next_id(Tick(98)), CueId::new(Tick(98), 1));

    // A tick nothing has been fired on starts at zero even when it sits between
    // two that have.
    assert_eq!(frame.next_id(Tick(0)), CueId::first(Tick(0)));
}

#[test]
fn next_id_is_a_function_of_the_frame_and_not_a_reservation() {
    let mut frame = AudioFrame::new();

    // Two calls with no push between them give the same answer. This is stated
    // in the documentation as a limit rather than a feature, and it is what
    // makes the numbering reproducible from a serialized frame alone.
    assert_eq!(frame.next_id(Tick(97)), frame.next_id(Tick(97)));

    frame.cue(Cue::new(CueId::new(Tick(97), 0), THUD));
    frame.cue(Cue::new(CueId::new(Tick(97), 0), THUD));

    // Nothing rejects the duplicate, and `next_id` reads the highest serial
    // present rather than counting entries, so it still says 1.
    assert_eq!(frame.cues.len(), 2);
    assert_eq!(frame.next_id(Tick(97)), CueId::new(Tick(97), 1));
}

#[test]
fn next_id_reads_the_highest_serial_rather_than_the_last_one_pushed() {
    let mut frame = AudioFrame::new();

    // A caller that pushed identities out of order — replaying a capture, say —
    // must not get a serial that collides with one already there.
    frame.cue(Cue::new(CueId::new(Tick(97), 5), THUD));
    frame.cue(Cue::new(CueId::new(Tick(97), 2), THUD));
    assert_eq!(frame.next_id(Tick(97)), CueId::new(Tick(97), 6));
}

#[test]
fn a_serial_at_the_ceiling_repeats_rather_than_wrapping() {
    let mut frame = AudioFrame::new();
    frame.cue(Cue::new(CueId::new(Tick(97), u16::MAX), THUD));

    // Wrapping would collide with the *first* cue of the tick, which is the one
    // most likely to still be playing. Repeating collides with the last, which
    // is the least bad answer available without panicking — and 65536 one-shots
    // in one tick is a bug upstream either way.
    assert_eq!(frame.next_id(Tick(97)), CueId::new(Tick(97), u16::MAX));
    assert_ne!(frame.next_id(Tick(97)), CueId::first(Tick(97)));
}

#[test]
fn the_builders_leave_everything_they_were_not_asked_about_alone() {
    let source = Source::new(SourceId(3), TORCH)
        .with_gain(Factor16::from_f64(0.5))
        .on(BusId(4));
    assert_eq!(source.id, SourceId(3));
    assert_eq!(source.sound, TORCH);
    assert_eq!(source.bus, BusId(4));
    assert_eq!(source.gain, Factor16::from_f64(0.5));
    assert_eq!(source.occlusion, Factor16::ZERO);

    // `with_gain` must not be reachable through `occluded_by` and vice versa.
    // They are the two `Factor16` fields on this type, so a builder that wrote
    // the wrong one would compile.
    let occluded = Source::new(SourceId(3), TORCH).occluded_by(Factor16::from_f64(0.5));
    assert_eq!(occluded.occlusion, Factor16::from_f64(0.5));
    assert_eq!(occluded.gain, Factor16::ONE);
    assert_ne!(occluded, source);
}
