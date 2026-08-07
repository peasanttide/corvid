//! The identity scheme, and only what it can be shown to do.
//!
//! Nothing in this crate mixes, so none of these tests can show that a
//! [`CueId`] is *sufficient* for a mixer. What they show is the four properties the
//! crate documentation claims and a mixer would be built on: an identity is
//! stable across two observations of one cue, distinct across ticks, distinct
//! across serials, and unmoved by every field of the payload. The decisions
//! left to a mixer are named in the README and are not tested here, because
//! nothing here could test them.

mod common;

use common::{THUD, extract};
use corvid_fixed::{Factor16, I8F8, I16F16};
use corvid_sound::{AudioFrame, BusId, Cue, CueId, SoundId};
use corvid_time::Tick;
use corvid_vector::FinePoint;
/// The identities in a frame, in the order they were emitted.
fn ids(frame: &AudioFrame) -> Vec<CueId> {
    frame.cues.iter().map(|cue| cue.id).collect()
}

#[test]
fn one_cue_observed_twice_keeps_its_identity_while_its_payload_moves() {
    // The simulation fired one bounce, on tick 97, at world x = 4. The client
    // extracts a frame once per *displayed* frame, so a fifteen-hertz tick is
    // extracted from nine or ten times — and between two of them the listener
    // walked from x = 0 to x = 1.
    let bounces = [(Tick(97), 4.0)];

    let mut early = AudioFrame::new();
    extract(&mut early, &bounces, 0.0);
    let mut late = AudioFrame::new();
    extract(&mut late, &bounces, 1.0);

    // The payload moved. Both fields of it: the offset and the gain.
    assert_ne!(early.cues[0].position, late.cues[0].position);
    assert_ne!(early.cues[0].gain, late.cues[0].gain);
    assert_ne!(early.cues[0], late.cues[0]);

    // The identity did not, which is the whole claim. A mixer comparing
    // payloads would have started this thud twice.
    assert_eq!(early.cues[0].id, late.cues[0].id);
    assert_eq!(ids(&early), ids(&late));
}

#[test]
fn two_cues_on_one_tick_are_distinguishable_from_one_cue_observed_twice() {
    // This is the pair the identity scheme exists to separate, so it is worth
    // making the two sides as similar as they can be made. Both bounces are the
    // same sound at the same place, so their payloads are *equal* — and the two
    // observations of the single bounce have payloads that *differ*. A scheme
    // that read the payload would get both of these exactly backwards.
    let mut two_on_one_tick = AudioFrame::new();
    extract(
        &mut two_on_one_tick,
        &[(Tick(97), 4.0), (Tick(97), 4.0)],
        0.0,
    );

    let mut observed_once = AudioFrame::new();
    extract(&mut observed_once, &[(Tick(97), 4.0)], 0.0);
    let mut observed_again = AudioFrame::new();
    extract(&mut observed_again, &[(Tick(97), 4.0)], 1.0);

    // Two cues: equal payloads, different identities.
    let (a, b) = (two_on_one_tick.cues[0], two_on_one_tick.cues[1]);
    assert_eq!(a.position, b.position);
    assert_eq!(a.gain, b.gain);
    assert_ne!(a.id, b.id);
    assert_eq!(a.id.serial, 0);
    assert_eq!(b.id.serial, 1);

    // One cue, twice: different payloads, one identity.
    let (once, again) = (observed_once.cues[0], observed_again.cues[0]);
    assert_ne!(once.position, again.position);
    assert_eq!(once.id, again.id);

    // And the counts, which is the question a mixer is really asking: how many
    // distinct one-shots have I been told about.
    let mut distinct: Vec<CueId> = ids(&two_on_one_tick);
    distinct.dedup();
    assert_eq!(distinct.len(), 2);

    let mut across_observations: Vec<CueId> = ids(&observed_once);
    across_observations.extend(ids(&observed_again));
    across_observations.dedup();
    assert_eq!(across_observations.len(), 1);
}

#[test]
fn two_cues_fired_on_different_ticks_are_distinguishable() {
    // Same sound, same place, same listener — everything but the tick.
    let mut frame = AudioFrame::new();
    extract(&mut frame, &[(Tick(97), 4.0), (Tick(98), 4.0)], 0.0);

    let (a, b) = (frame.cues[0], frame.cues[1]);
    assert_eq!(a.position, b.position);
    assert_eq!(a.gain, b.gain);
    assert_ne!(a.id, b.id);
    assert_eq!(a.id.fired, Tick(97));
    assert_eq!(b.id.fired, Tick(98));

    // Both are serial zero, so the serial alone would have merged them. It is
    // the pair that is the identity.
    assert_eq!(a.id.serial, b.id.serial);
}

#[test]
fn a_rollback_that_unfires_a_cue_removes_its_identity() {
    // Tick 95 through 98 ran with a predicted action and produced three
    // bounces. The correction arrives, a runtime re-simulates, and the bounce on
    // 97 no longer happens. This crate has no runtime in it — a rollback is
    // `corvid_replay::Session::seek`, two rings up — so the two frames are stood
    // side by side rather than produced by rolling one back: what is under test
    // is what the identities do, not what a rollback does.
    let predicted = [(Tick(95), 1.0), (Tick(97), 4.0), (Tick(98), 2.0)];
    let corrected = [(Tick(95), 1.0), (Tick(98), 2.0)];

    let mut before = AudioFrame::new();
    extract(&mut before, &predicted, 0.0);
    let mut after = AudioFrame::new();
    extract(&mut after, &corrected, 0.0);

    let gone = CueId::first(Tick(97));
    assert!(ids(&before).contains(&gone));
    assert!(!ids(&after).contains(&gone));

    // What survived kept its identity, which is what makes the difference
    // readable as "one cue vanished" rather than "everything renumbered". Ticks
    // 95 and 98 each fired one cue, so removing the one between them does not
    // move either serial.
    assert!(ids(&after).contains(&CueId::first(Tick(95))));
    assert!(ids(&after).contains(&CueId::first(Tick(98))));

    // Whether a mixer cuts the thud it has already started, ducks it, or lets
    // it ring out is the mixer's decision. Nothing here makes it, and nothing
    // here could.
}

#[test]
fn a_rollback_that_refires_a_cue_gives_back_the_same_identity() {
    // The other direction: the re-simulation produces the same bounce again.
    // The identity is a function of the simulation alone, so it comes back
    // identical — and it comes back identical even though the listener has
    // moved in the meantime, which is what makes it recognisable at all.
    let bounces = [(Tick(95), 1.0), (Tick(97), 4.0)];

    let mut first_pass = AudioFrame::new();
    extract(&mut first_pass, &bounces, 0.0);
    let mut second_pass = AudioFrame::new();
    extract(&mut second_pass, &bounces, 3.0);

    assert_eq!(ids(&first_pass), ids(&second_pass));
    assert_ne!(first_pass.cues, second_pass.cues);

    // And the case the README says a mixer has to decide about: a
    // re-simulation that produced a *different* payload under an identity the
    // mixer has already started. It is detectable, because `PartialEq` covers
    // the payload; what to do about it is not something this crate decides.
    let held = first_pass.cues[1];
    let arrived = second_pass.cues[1];
    assert_eq!(held.id, arrived.id);
    assert_ne!(held, arrived);
}

#[test]
fn no_field_of_the_payload_reaches_the_identity() {
    // One at a time, because a single "everything different" cue would pass
    // against an identity that read any one field.
    let base = Cue::new(CueId::new(Tick(97), 3), THUD);
    let variants = [
        Cue {
            sound: SoundId(9),
            ..base
        },
        Cue {
            bus: BusId(9),
            ..base
        },
        Cue {
            position: FinePoint::new(I16F16::ONE, I16F16::ONE, I16F16::ONE),
            ..base
        },
        Cue {
            gain: Factor16::ZERO,
            ..base
        },
        Cue {
            pitch: I8F8::ZERO,
            ..base
        },
    ];

    for variant in variants {
        assert_ne!(
            variant, base,
            "a variant that changed nothing proves nothing"
        );
        assert_eq!(variant.id, base.id);
    }
}

#[test]
fn identities_order_by_tick_first_and_serial_second() {
    // A mixer keeping what it has started in an ordered map wants "everything
    // before tick 95" to be a range query, which it is only if the tick is the
    // major key.
    let mut sorted = [
        CueId::new(Tick(98), 0),
        CueId::new(Tick(97), 1),
        CueId::new(Tick(97), 0),
        CueId::new(Tick(95), 9),
    ];
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        [
            CueId::new(Tick(95), 9),
            CueId::new(Tick(97), 0),
            CueId::new(Tick(97), 1),
            CueId::new(Tick(98), 0),
        ]
    );

    // Spelled out, so that a derive reordered to `(serial, fired)` fails here
    // rather than only in the sort above: a high serial on an early tick still
    // sorts before a low serial on a late one.
    assert!(CueId::new(Tick(95), 9) < CueId::new(Tick(97), 0));
}

#[test]
fn shuffling_the_extractors_output_moves_the_serials() {
    // The obligation the crate documentation puts on an extractor, made
    // visible. A serial is a position in the emission order, so an extractor
    // that iterates a hash map hands a mixer two different identities for one
    // bounce on consecutive frames.
    let mut forwards = AudioFrame::new();
    extract(&mut forwards, &[(Tick(97), 1.0), (Tick(97), 4.0)], 0.0);
    let mut backwards = AudioFrame::new();
    extract(&mut backwards, &[(Tick(97), 4.0), (Tick(97), 1.0)], 0.0);

    assert_eq!(ids(&forwards), ids(&backwards));
    assert_ne!(forwards.cues[0], backwards.cues[0]);

    // Which is the point: the identities matched and the payloads did not, so
    // the bounce at x = 4 was cue 97#1 in one frame and 97#0 in the other. The
    // crate cannot prevent this and does not claim to.
    assert_eq!(forwards.cues[1].position, backwards.cues[0].position);
    assert_eq!(forwards.cues[1].gain, backwards.cues[0].gain);
    assert_ne!(forwards.cues[1].id, backwards.cues[0].id);
}
