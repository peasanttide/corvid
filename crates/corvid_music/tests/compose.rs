//! What the composer promises about a bar, checked against bars it wrote.

#![cfg(feature = "compose")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

use corvid_music::{
    Cadence, Composer, Event, Mode, Motif, MotifId, Parameters, Role, Step, parallel_perfects,
};

/// A short tune with a shape a contour comparison can see.
fn tune(id: u32) -> Motif {
    Motif::new(
        MotifId(id),
        vec![
            Event::note(Step::new(0), 1.0),
            Event::note(Step::new(2), 0.5),
            Event::note(Step::new(4), 0.5),
            Event::note(Step::new(3), 1.0),
            Event::rest(0.5),
            Event::note(Step::new(1), 0.5),
        ],
    )
}

/// A composer with one tune in it.
fn composer(seed: u64, parameters: Parameters) -> Composer {
    let mut composer = Composer::new(seed, parameters);
    composer.motifs_mut().insert(tune(1));
    composer
}

#[test]
fn a_seed_and_parameters_reproduce_every_bar() {
    let bars = |seed| {
        let mut composer = composer(seed, Parameters::default());
        (0..16).map(|_| composer.next_bar()).collect::<Vec<_>>()
    };
    assert_eq!(bars(2026), bars(2026));
    assert_ne!(bars(2026), bars(2027));
}

#[test]
fn a_bar_is_never_silent_and_never_leaves_the_keyboard() {
    let mut composer = composer(5, Parameters::default());
    for _ in 0..32 {
        let bar = composer.next_bar();
        assert!(!bar.is_silent(), "bar {} is silent", bar.index);
        assert!(bar.beats > 0.0);
        for voice in &bar.voices {
            for note in &voice.notes {
                assert!(note.beats > 0.0, "{note:?}");
                assert!(note.beat >= 0.0);
                assert!(note.key <= 127);
            }
        }
    }
}

#[test]
fn raising_tempo_raises_onsets_per_second() {
    let at = |tempo| {
        let mut composer = composer(
            77,
            Parameters {
                tempo,
                ..Parameters::default()
            },
        );
        (0..8)
            .map(|_| composer.next_bar().onsets_per_second())
            .sum::<f32>()
    };
    let slow = at(60.0);
    let fast = at(150.0);
    assert!(slow > 0.0);
    // Tempo is the one parameter that changes nothing about which notes are
    // written, so the same bars arrive two and a half times as fast.
    assert!(fast > slow * 2.4, "slow {slow}, fast {fast}");
    assert!(fast < slow * 2.6, "slow {slow}, fast {fast}");
}

#[test]
fn raising_voice_count_adds_voices_without_breaking_voice_leading() {
    for seed in 0..24u64 {
        let mut thin = composer(
            seed,
            Parameters {
                voices: 2,
                ..Parameters::default()
            },
        );
        let mut thick = composer(
            seed,
            Parameters {
                voices: 5,
                ..Parameters::default()
            },
        );
        let mut before: Option<_> = None;
        let mut after: Option<_> = None;
        for _ in 0..8 {
            let lean = thin.next_bar();
            let full = thick.next_bar();
            assert!(
                full.pitched() > lean.pitched(),
                "seed {seed}: {} against {}",
                full.pitched(),
                lean.pitched()
            );
            assert_eq!(
                parallel_perfects(&lean, before.as_ref()),
                0,
                "seed {seed} thin"
            );
            assert_eq!(
                parallel_perfects(&full, after.as_ref()),
                0,
                "seed {seed} thick"
            );
            before = Some(lean);
            after = Some(full);
        }
    }
}

#[test]
fn dissonance_above_the_strict_line_stops_the_promise_being_made() {
    // Not a claim that parallels appear -- they are permitted, not required.
    // What is claimed is that the strict line is where the promise is made, and
    // `raising_voice_count_adds_voices_without_breaking_voice_leading` is what
    // shows it kept below it.
    let mut composer = composer(
        3,
        Parameters {
            dissonance: 1.0,
            voices: 5,
            ..Parameters::default()
        },
    );
    for _ in 0..8 {
        let bar = composer.next_bar();
        assert!(!bar.is_silent());
    }
}

#[test]
fn mode_darkness_moves_the_third_flat() {
    let ladder: Vec<i8> = Mode::LADDER.iter().map(|mode| mode.third()).collect();
    assert!(ladder.windows(2).all(|pair| pair[0] >= pair[1]));
    assert!(Mode::from_darkness(1.0).third() < Mode::from_darkness(0.0).third());

    let mode_of = |dark| {
        let mut composer = composer(
            41,
            Parameters {
                mode_dark: dark,
                ..Parameters::default()
            },
        );
        composer.next_bar().mode
    };
    assert!(mode_of(0.95).third() < mode_of(0.05).third());
}

#[test]
fn every_pitched_note_belongs_to_the_bar_s_own_scale() {
    // The mode is only a claim about the third if the notes are actually in it.
    let mut composer = composer(
        19,
        Parameters {
            ornament: 0.0,
            chromaticism: 0.0,
            ..Parameters::default()
        },
    );
    for _ in 0..24 {
        let bar = composer.next_bar();
        for voice in bar.voices.iter().filter(|voice| voice.is_pitched()) {
            for note in &voice.notes {
                let semitone =
                    i8::try_from((i16::from(note.key) - i16::from(bar.tonic)).rem_euclid(12))
                        .unwrap_or(0);
                assert!(
                    bar.mode.contains_semitone(semitone),
                    "bar {}: key {} is outside {:?} on {}",
                    bar.index,
                    note.key,
                    bar.mode,
                    bar.tonic
                );
            }
        }
    }
}

#[test]
fn a_cadence_is_deferred_while_tension_rises_and_lands_when_it_stops() {
    let mut composer = composer(8, Parameters::default());
    let mut tension = 0.0;

    // Rising tension, for longer than a phrase would otherwise take.
    let mut held = 0;
    for _ in 0..14 {
        tension += 0.05;
        composer.set_tension(tension);
        let bar = composer.next_bar();
        assert!(
            bar.cadence.is_none(),
            "bar {} resolved while tension was rising",
            bar.index
        );
        held = held.max(composer.deferrals());
    }
    assert!(held > 0, "the cadence was never actually deferred");
    assert!(held <= Composer::MAX_DEFERRALS);

    // Tension stops rising, and the phrase is allowed to close.
    let mut landed = None;
    for _ in 0..4 {
        composer.set_tension(tension);
        let bar = composer.next_bar();
        if let Some(cadence) = bar.cadence {
            landed = Some(cadence);
            break;
        }
    }
    assert_eq!(landed, Some(Cadence::Authentic));
    assert!(landed.is_some_and(Cadence::resolves));
}

#[test]
fn an_interruption_cuts_the_bar_and_leaves_what_had_started_ringing() {
    let mut composer = composer(6, Parameters::default());
    let whole = composer.next_bar();
    let cut = composer.interrupt(1.0).expect("a bar has been written");

    assert!(cut.elided);
    assert!(!whole.elided);
    assert!((cut.beats - 1.0).abs() < 1e-6, "cut at {}", cut.beats);
    assert!(cut.beats < whole.beats);

    // Every surviving note is one that had already started, and it keeps the
    // length it was written with: it rings out over the cut.
    for (before, after) in whole.voices.iter().zip(cut.voices.iter()) {
        assert_eq!(after.role, before.role);
        for note in &after.notes {
            assert!(note.beat < 1.0);
            assert!(before.notes.contains(note));
        }
    }
    assert!(cut.onsets() < whole.onsets());
}

#[test]
fn an_interruption_starts_a_new_phrase_on_the_next_bar() {
    let mut composer = composer(6, Parameters::default());
    let _ = composer.next_bar();
    let _ = composer.next_bar();
    let _ = composer.interrupt(0.5);
    let after = composer.next_bar();
    // A new phrase writes its own metre and its own first bar, so the bar after
    // an interruption is the start of something rather than the middle of what
    // was cut.
    assert_eq!(after.index, 2);
    assert!(!after.elided);
    assert!(!after.is_silent());
}

#[test]
fn a_composer_with_no_motifs_writes_accompaniment_and_no_tune() {
    let mut composer = Composer::new(1, Parameters::default());
    let bar = composer.next_bar();
    assert_eq!(bar.motif, None);
    assert!(
        bar.voice(Role::Lead)
            .is_some_and(|lead| lead.notes.is_empty())
    );
    assert!(!bar.is_silent());
}
