//! Writing a bar into an audio frame, in that crate's own vocabulary.

#![cfg(feature = "sound")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

use corvid_fixed::{Factor16, I8F8};
use corvid_music::{
    Composer, Event, Motif, MotifId, Parameters, Role, Step, Timbre, music_bus, write_bar,
};
use corvid_sound::{AudioFrame, BusId, Cue, SoundId};
use corvid_time::Tick;

/// The bus the score plays on.
const MUSIC: BusId = BusId(3);
/// How many ticks a bar lasts in these tests.
const TICKS_PER_BAR: u32 = 30;

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

/// A catalogue that answers for the tune and the bass and nothing else.
fn catalogue(role: Role) -> Option<Timbre> {
    match role {
        Role::Lead => Some(Timbre::new(SoundId(1), 72)),
        Role::Bass => Some(Timbre::new(SoundId(2), 48)),
        _ => None,
    }
}

#[test]
fn the_score_gets_a_bus_of_its_own_under_the_master() {
    let bus = music_bus(MUSIC, Factor16::from_f64(0.6));
    assert_eq!(bus.id, MUSIC);
    assert_eq!(bus.parent, Some(BusId::MASTER));
    assert_eq!(bus.gain, Factor16::from_f64(0.6));
}

#[test]
fn every_note_of_a_bar_becomes_a_cue_on_that_bus() {
    let bar = composer(21).next_bar();
    let mut frame = AudioFrame::new();
    frame.bus(music_bus(MUSIC, Factor16::ONE));
    let written = write_bar(&bar, MUSIC, Tick(60), TICKS_PER_BAR, &mut frame, catalogue);

    let expected: usize = bar
        .voices
        .iter()
        .filter(|voice| catalogue(voice.role).is_some())
        .map(|voice| voice.notes.len())
        .sum();
    assert_eq!(written, expected);
    assert_eq!(frame.cues.len(), written);
    assert!(written > 0);

    for cue in &frame.cues {
        assert_eq!(cue.bus, MUSIC);
        // The score has no position: it neither pans nor occludes, which is
        // exactly what makes it the score rather than a street singer. Compared
        // against a cue built at the listener rather than against a point this
        // file would have to name a fourth crate to spell.
        assert_eq!(cue.position, Cue::new(cue.id, cue.sound).position);
        assert!(cue.id.fired >= Tick(60));
        assert!(cue.id.fired < Tick(60 + u64::from(TICKS_PER_BAR) + 1));
        assert!(cue.pitch > I8F8::ZERO);
    }
}

#[test]
fn a_role_the_catalogue_has_no_recording_for_is_silent() {
    let bar = composer(22).next_bar();
    let mut frame = AudioFrame::new();
    let written = write_bar(&bar, MUSIC, Tick(0), TICKS_PER_BAR, &mut frame, |_| None);
    assert_eq!(written, 0);
    assert!(frame.is_empty());
}

#[test]
fn a_cue_s_identity_is_the_tick_it_fell_on_and_its_place_in_that_tick() {
    let bar = composer(23).next_bar();
    let mut frame = AudioFrame::new();
    let _ = write_bar(&bar, MUSIC, Tick(100), TICKS_PER_BAR, &mut frame, catalogue);

    // Serials are assigned from what is already in the frame, so two cues that
    // land on the same tick are numbered apart -- which is the whole of what
    // makes a rollback able to tell them apart.
    let mut seen: Vec<_> = frame.cues.iter().map(|cue| cue.id).collect();
    let count = seen.len();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), count, "two cues shared an identity");
}

#[test]
fn a_recording_is_resampled_to_the_key_that_is_wanted() {
    let timbre = Timbre::new(SoundId(1), 60);
    assert_eq!(timbre.rate(60), I8F8::ONE);
    // An octave up is twice the rate, and an octave down is half of it.
    assert_eq!(timbre.rate(72), I8F8::from_f64(2.0));
    assert_eq!(timbre.rate(48), I8F8::from_f64(0.5));
    assert!(timbre.rate(127) > I8F8::ONE);
    assert!(timbre.rate(0) < I8F8::ONE);
}
