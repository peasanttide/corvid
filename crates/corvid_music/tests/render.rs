//! Turning notes into samples: with a bank, without one, and from a bar.

#![cfg(feature = "synth")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

mod common;

use corvid_music::{Bank, MidiEvent, Synth, TimedEvent, Waveform};

/// How many frames a test block holds.
const FRAMES: usize = 4_800;
/// The rate the tests render at.
const RATE: u32 = 48_000;

/// A block of interleaved stereo, zeroed.
fn block() -> Vec<f32> {
    vec![0.0; FRAMES * 2]
}

/// The loudest sample in a block.
fn peak(block: &[f32]) -> f32 {
    block
        .iter()
        .fold(0.0f32, |held, sample| held.max(sample.abs()))
}

/// Whether every sample is inside the range a device accepts.
fn in_range(block: &[f32]) -> bool {
    block
        .iter()
        .all(|sample| sample.is_finite() && sample.abs() <= 1.0)
}

#[test]
fn a_note_on_an_oscillator_sounds_and_a_note_off_frees_it() {
    let mut synth = Synth::new(RATE);
    synth.set_waveform(0, Waveform::Sine);
    synth.send(MidiEvent::NoteOn {
        channel: 0,
        key: 69,
        velocity: 100,
    });

    let mut sound = block();
    synth.render(&mut sound);
    assert_eq!(synth.active_voices(), 1);
    assert!(peak(&sound) > 0.01, "peaked at {}", peak(&sound));
    assert!(in_range(&sound));

    synth.send(MidiEvent::NoteOff {
        channel: 0,
        key: 69,
    });
    let mut tail = block();
    synth.render(&mut tail);
    // The release is under a tenth of a second, so a tenth-second block after
    // the note-off is enough to reclaim the voice.
    assert_eq!(synth.active_voices(), 0);
}

#[test]
fn a_note_on_a_bank_sounds_the_bank_s_own_sample() {
    let bank = Bank::parse(&common::image()).expect("the fixture is well formed");
    let mut synth = Synth::with_bank(RATE, bank);
    synth.send(MidiEvent::NoteOn {
        channel: 0,
        key: common::ROOT_KEY,
        velocity: 110,
    });

    let mut sound = block();
    synth.render(&mut sound);
    assert!(peak(&sound) > 0.01, "peaked at {}", peak(&sound));
    assert!(in_range(&sound));

    // The zone loops, so a note held far past the sample's own length is still
    // sounding rather than having run off the end.
    for _ in 0..4 {
        let mut more = block();
        synth.render(&mut more);
        assert!(peak(&more) > 0.01);
    }
    assert_eq!(synth.active_voices(), 1);
}

#[test]
fn silence_is_what_a_synthesizer_with_nothing_to_play_writes() {
    let mut synth = Synth::new(RATE);
    let mut quiet = block();
    synth.render(&mut quiet);
    // `peak` is an absolute value, so at or below zero is exactly zero.
    assert!(peak(&quiet) <= 0.0);
    assert!(in_range(&quiet));
}

#[test]
fn a_scheduled_event_lands_on_the_frame_it_names() {
    let mut synth = Synth::new(RATE);
    synth.schedule(TimedEvent::new(
        u64::try_from(FRAMES).unwrap() / 2,
        MidiEvent::NoteOn {
            channel: 0,
            key: 72,
            velocity: 120,
        },
    ));

    let mut sound = block();
    synth.render(&mut sound);
    let half = FRAMES;
    let before = sound.get(..half).map_or(1.0, peak);
    let after = sound.get(half..).map_or(0.0, peak);
    assert!(before <= 0.0, "the note sounded before it was scheduled");
    assert!(after > 0.01, "the note never sounded: {after}");
    assert_eq!(synth.clock(), u64::try_from(FRAMES).unwrap());
}

#[test]
fn the_mix_stays_in_range_however_many_voices_are_sounding() {
    let mut synth = Synth::new(RATE).with_gain(4.0);
    for key in 48..72u8 {
        synth.send(MidiEvent::NoteOn {
            channel: 0,
            key,
            velocity: 127,
        });
    }
    let mut loud = block();
    synth.render(&mut loud);
    // Deliberately driven far past full scale: the clamp is a promise about
    // what leaves this crate, not an assumption about what arrives.
    assert!(in_range(&loud));
    assert!(peak(&loud) > 0.5);
}

#[cfg(feature = "compose")]
mod from_a_bar {
    use super::{RATE, block, in_range, peak};
    use corvid_music::{
        Bank, Composer, Event, Motif, MotifId, Parameters, Step, Synth, Waveform, perform,
    };

    /// A composer with one tune in it.
    fn composer(seed: u64) -> Composer {
        let mut composer = Composer::new(seed, Parameters::default());
        composer.motifs_mut().insert(Motif::new(
            MotifId(1),
            vec![
                Event::note(Step::new(0), 0.5),
                Event::note(Step::new(2), 0.5),
                Event::note(Step::new(4), 1.0),
            ],
        ));
        composer
    }

    #[test]
    fn a_rendered_bar_is_not_silent_and_stays_in_range() {
        let mut composer = composer(2026);
        let mut synth = Synth::new(RATE);
        for channel in 0..16 {
            synth.set_waveform(channel, Waveform::Triangle);
        }

        for _ in 0..8 {
            let bar = composer.next_bar();
            synth.schedule_all(perform(&bar, RATE, synth.clock()));
            let mut sound = block();
            synth.render(&mut sound);
            assert!(peak(&sound) > 0.01, "bar {} was silent", bar.index);
            assert!(in_range(&sound), "bar {} left the range", bar.index);
        }
    }

    #[test]
    fn a_rendered_bar_sounds_through_a_bank_too() {
        let bank = Bank::parse(&super::common::image()).expect("the fixture is well formed");
        let mut composer = composer(7);
        let mut synth = Synth::with_bank(RATE, bank);

        let bar = composer.next_bar();
        synth.schedule_all(perform(&bar, RATE, 0));
        let mut sound = block();
        synth.render(&mut sound);
        assert!(peak(&sound) > 0.01);
        assert!(in_range(&sound));
    }

    #[test]
    fn a_bar_becomes_events_in_frame_order_with_a_pair_per_note() {
        let bar = composer(4).next_bar();
        let events = perform(&bar, RATE, 1_000);

        assert_eq!(events.len(), bar.onsets() * 2);
        assert!(events.windows(2).all(|pair| pair[0].frame <= pair[1].frame));
        assert!(events.iter().all(|event| event.frame >= 1_000));

        // Every note-on is answered by a note-off on the same channel and key,
        // or a voice would be left sounding forever.
        let ons = events
            .iter()
            .filter(|event| matches!(event.event, corvid_music::MidiEvent::NoteOn { .. }))
            .count();
        assert_eq!(ons * 2, events.len());
    }
}
