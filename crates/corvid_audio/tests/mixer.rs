//! What a pool of voices does when it is full, loud, or empty.
//!
//! Every one of these is a property of the half of the crate a device drives,
//! checked without a device. That is the whole reason the mixer is a type of
//! its own rather than a closure inside the backend.

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::float_cmp,
    reason = "silence is exactly zero and a clipped sample is exactly one; a tolerance on either would pass on a mixer that had written almost-silence over a buffer it was meant to fill"
)]

use corvid_audio::{Mixer, Note, Timbre};

/// The rate every fixture mixes at.
const RATE: u32 = 48_000;

/// A note at `gain` that rings for a good long while.
const fn ringing(gain: f32) -> Note {
    Note {
        timbre: Timbre::knock(440.0).with_decay(4.0),
        gain,
    }
}

/// The loudest sample in `samples` of a fresh buffer.
fn peak(mixer: &mut Mixer, samples: usize) -> f32 {
    let mut buffer = vec![0.0f32; samples];
    mixer.fill(&mut buffer, 1);
    buffer
        .iter()
        .fold(0.0f32, |loudest, sample| loudest.max(sample.abs()))
}

#[test]
fn the_half_with_no_device_in_it_crosses_threads() {
    // The counterpart of the `compile_fail` on `Audio`, which asserts that the
    // *device* half cannot leave the thread that opened it. On its own that
    // snippet would pass against a name that does not exist, a trait bound
    // written wrong, or an unrelated syntax error — so this is the same
    // assertion, in the same crate, about types that are expected to compile.
    // The mixing arithmetic runs on whatever thread a device hands it, and
    // `Note` and `Timbre` are what crosses to it.
    fn crosses_threads<T: Send>() {}
    crosses_threads::<Mixer>();
    crosses_threads::<Note>();
    crosses_threads::<Timbre>();
}

#[test]
fn a_mixer_with_nothing_playing_writes_silence_over_what_was_there() {
    // A device buffer holds whatever was in it last time. A mixer that skipped
    // the write when nothing was playing would repeat the last few
    // milliseconds forever, which is a stuck note rather than a silence.
    let mut mixer = Mixer::new(RATE, 8);
    let mut buffer = [0.7f32; 64];
    mixer.fill(&mut buffer, 1);
    assert!(buffer.iter().all(|sample| *sample == 0.0));
    assert_eq!(mixer.playing(), 0);
}

#[test]
fn every_channel_gets_the_same_sample() {
    // The spatialization stub, at the buffer rather than at the frame: an
    // interleaved stereo buffer is the same value twice, and the day a panner
    // lands this is the test that says so.
    let mut mixer = Mixer::new(RATE, 8);
    mixer.start(ringing(1.0));
    let mut buffer = vec![0.0f32; 512];
    mixer.fill(&mut buffer, 2);
    for pair in buffer.chunks(2) {
        assert_eq!(pair.first(), pair.last());
    }
    assert!(buffer.iter().any(|sample| *sample != 0.0));
}

#[test]
fn a_free_voice_is_used_before_a_playing_one_is_stolen() {
    let mut mixer = Mixer::new(RATE, 4);
    for _ in 0..4 {
        mixer.start(ringing(0.2));
    }
    assert_eq!(mixer.playing(), 4);
    assert_eq!(mixer.stolen(), 0);
}

#[test]
fn a_full_pool_steals_the_quietest_voice_rather_than_the_oldest() {
    // The oldest voice here is the loudest, and stealing it is what a
    // first-in-first-out pool would do — which cuts a sound the player is
    // listening to in favour of one they have not heard yet.
    let mut mixer = Mixer::new(RATE, 2);
    mixer.start(ringing(1.0));
    mixer.start(ringing(0.01));
    // Let both settle so their loudness is unmistakably different.
    let _ = peak(&mut mixer, 480);

    // A newcomer quiet enough that the mix is unmistakably one or the other:
    // with the loud voice still playing the peak is most of full scale, and
    // with it stolen the two remaining voices together cannot reach a twentieth
    // of it.
    mixer.start(ringing(0.02));
    assert_eq!(mixer.stolen(), 1);
    assert_eq!(mixer.playing(), 2);

    let after = peak(&mut mixer, 480);
    assert!(
        after > 0.5,
        "the loudest voice was stolen; the peak fell to {after}"
    );
}

#[test]
fn the_sum_of_many_loud_voices_is_clipped_rather_than_wrapped() {
    // Sixteen voices at full gain sum well past full scale. What matters is
    // that the buffer stays inside the range a device can play: a sample
    // outside it is not a loud sound, it is whatever the conversion does with
    // an out-of-range float.
    let mut mixer = Mixer::new(RATE, 16);
    for _ in 0..16 {
        mixer.start(ringing(1.0));
    }
    let mut buffer = vec![0.0f32; 4_096];
    mixer.fill(&mut buffer, 1);
    assert!(buffer.iter().all(|sample| sample.abs() <= 1.0));
    assert!(
        buffer
            .iter()
            .any(|sample| (sample.abs() - 1.0).abs() < 1e-6),
        "sixteen voices at full gain never reached full scale, so nothing clipped",
    );
}

#[test]
fn a_voice_that_has_decayed_frees_its_slot_without_being_stolen() {
    let mut mixer = Mixer::new(RATE, 4);
    mixer.start(Note {
        timbre: Timbre::knock(440.0).with_decay(0.01),
        gain: 1.0,
    });
    assert_eq!(mixer.playing(), 1);
    let _ = peak(&mut mixer, RATE as usize / 4);
    assert_eq!(mixer.playing(), 0);
    assert_eq!(mixer.stolen(), 0);
}

#[test]
fn a_mixer_asked_for_no_voices_still_has_one() {
    // Zero would be a pool with nowhere to start a note, and a game that asked
    // for it would get silence with no error to explain it.
    let mut mixer = Mixer::new(RATE, 0);
    mixer.start(ringing(1.0));
    assert_eq!(mixer.playing(), 1);
}

#[test]
fn a_device_that_reports_no_sample_rate_still_produces_finite_samples() {
    // Zero is what a device that could not say reports, and it is a divisor
    // everywhere in the oscillator.
    let mut mixer = Mixer::new(0, 4);
    mixer.start(ringing(1.0));
    let mut buffer = vec![0.0f32; 256];
    mixer.fill(&mut buffer, 1);
    assert!(buffer.iter().all(|sample| sample.is_finite()));
    assert!(mixer.rate() >= 1.0);
}

#[test]
fn silencing_stops_everything_at_once() {
    let mut mixer = Mixer::new(RATE, 8);
    for _ in 0..8 {
        mixer.start(ringing(1.0));
    }
    mixer.silence();
    assert_eq!(mixer.playing(), 0);
    let mut buffer = [0.5f32; 32];
    mixer.fill(&mut buffer, 1);
    assert!(buffer.iter().all(|sample| *sample == 0.0));
}
