//! Motif memory: a theme that recurs, and recurs as itself.

#![cfg(feature = "compose")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

use corvid_music::{
    Bar, Composer, Event, Mode, Motif, MotifId, Note, Parameters, Role, Step, Subject, Transform,
    contour_similarity, transform,
};

/// The subject the tune under test is about.
const EFFIGY: Subject = Subject(1);
/// The tune under test.
const THEME: MotifId = MotifId(1);

/// A tune with a shape: up, up, down, up.
fn theme() -> Motif {
    Motif::new(
        THEME,
        vec![
            Event::note(Step::new(0), 0.5),
            Event::note(Step::new(2), 0.5),
            Event::note(Step::new(4), 0.5),
            Event::note(Step::new(3), 0.5),
            Event::note(Step::new(5), 1.0),
        ],
    )
    .about(EFFIGY)
}

/// Three other tunes, so the pool has something else to offer.
fn distractions() -> impl Iterator<Item = Motif> {
    (2..5u32).map(|id| {
        Motif::new(
            MotifId(id),
            vec![
                Event::note(Step::new(6), 1.0),
                Event::note(Step::new(5), 1.0),
                Event::note(Step::new(4), 2.0),
            ],
        )
    })
}

/// The scale degrees of a bar's lead, in order.
fn degrees(bar: &Bar) -> Vec<u8> {
    let Some(lead) = bar.voice(Role::Lead) else {
        return Vec::new();
    };
    lead.notes
        .iter()
        .filter_map(|note| degree_of(note.key, bar.tonic, bar.mode))
        .collect()
}

/// Which degree of `mode` on `tonic` the key is, if it is one.
fn degree_of(key: u8, tonic: u8, mode: Mode) -> Option<u8> {
    let semitone = i8::try_from((i16::from(key) - i16::from(tonic)).rem_euclid(12)).ok()?;
    mode.semitones()
        .iter()
        .position(|held| *held == semitone)
        .and_then(|index| u8::try_from(index).ok())
}

/// The lead line of a bar.
fn lead(bar: &Bar) -> Vec<Note> {
    bar.voice(Role::Lead)
        .map(|v| v.notes.clone())
        .unwrap_or_default()
}

#[test]
fn a_warmed_motif_recurs_and_recurs_as_itself() {
    let mut composer = Composer::new(
        99,
        Parameters {
            // No decoration, so what the lead plays is the tune and nothing
            // else. Decoration is tested where decoration is the subject.
            ornament: 0.0,
            ..Parameters::default()
        },
    );
    composer.motifs_mut().insert(theme());
    for motif in distractions() {
        composer.motifs_mut().insert(motif);
    }

    let mut occurrences: Vec<Bar> = Vec::new();
    let mut last: Option<MotifId> = None;
    for _ in 0..64 {
        // The effigy is present every bar, so the theme stays hot while the
        // pool cools everything else.
        composer.motifs_mut().warm(EFFIGY, 1.0);
        let bar = composer.next_bar();
        if bar.motif == Some(THEME) && last != Some(THEME) && bar.variation == 0 {
            occurrences.push(bar.clone());
        }
        last = bar.motif;
    }

    assert!(
        occurrences.len() >= 2,
        "the theme was quoted {} time(s) in 64 bars",
        occurrences.len()
    );

    // Recurs *as itself*: every note the lead plays is the next degree of the
    // tune the pack wrote, so the quotation is exact and not merely similar. A
    // bar longer than the tune goes round again rather than inventing an
    // ending, so what a bar quotes is the start of the tune repeated.
    let written: Vec<u8> = theme()
        .events
        .iter()
        .filter_map(|event| event.step)
        .filter_map(|step| u8::try_from(step.degree).ok())
        .collect();
    for bar in &occurrences {
        let quoted = degrees(bar);
        assert!(!quoted.is_empty(), "bar {} quoted nothing", bar.index);
        let expected: Vec<u8> = written
            .iter()
            .copied()
            .cycle()
            .take(quoted.len().max(written.len()))
            .collect();
        assert!(
            expected.starts_with(&quoted),
            "bar {} played {quoted:?}, which is not the start of {expected:?}",
            bar.index
        );
    }

    // And recognisably so: the two occurrences rise and fall together, over
    // however much of the tune each bar's metre had room for.
    let first = lead(&occurrences[0]);
    let second = lead(&occurrences[1]);
    let likeness = contour_similarity(&first, &second);
    assert!(likeness > 0.4, "the recurrence scored {likeness}");
}

#[test]
fn heat_is_what_decides_which_tune_is_drawn() {
    let count = |warm: bool| {
        let mut composer = Composer::new(4, Parameters::default());
        composer.motifs_mut().insert(theme());
        for motif in distractions() {
            composer.motifs_mut().insert(motif);
        }
        let mut quoted = 0;
        for _ in 0..64 {
            if warm {
                composer.motifs_mut().warm(EFFIGY, 4.0);
            }
            if composer.next_bar().motif == Some(THEME) {
                quoted += 1;
            }
        }
        quoted
    };
    let hot = count(true);
    let cold = count(false);
    assert!(hot > cold, "hot {hot}, cold {cold}");
}

#[test]
fn a_cold_pool_still_answers() {
    // Nothing has ever been warmed, so every weight is the floor -- and a
    // composer that refused to quote anything because nothing was hot would be
    // silent for the whole of a calm level.
    let mut composer = Composer::new(12, Parameters::default());
    for motif in distractions() {
        composer.motifs_mut().insert(motif);
    }
    assert!(composer.next_bar().motif.is_some());
}

#[test]
fn cooling_lets_a_subject_fade() {
    let mut composer = Composer::new(1, Parameters::default());
    composer.motifs_mut().insert(theme().with_heat(1.0));
    // `next_bar` cools the pool once a bar, so a subject nobody names again
    // loses its advantage over a phrase rather than at the next barline.
    let before = composer.motifs().get(THEME).map(|motif| motif.heat);
    for _ in 0..8 {
        let _ = composer.next_bar();
    }
    let after = composer.motifs().get(THEME).map(|motif| motif.heat);
    assert!(after < before);
    assert!(after.is_some_and(|heat| heat > 0.0));
}

#[test]
fn every_transformation_stays_in_the_scale_and_keeps_its_alterations() {
    let phrase = [
        Event::note(Step::new(0), 1.0),
        Event::note(Step::new(2).altered(-1), 1.0),
        Event::rest(0.5),
        Event::note(Step::new(4), 0.5),
    ];
    for chain in [
        vec![Transform::Transpose(3)],
        vec![Transform::Invert],
        vec![Transform::Retrograde],
        vec![Transform::Augment],
        vec![Transform::Diminish],
        vec![Transform::Invert, Transform::Transpose(-2)],
    ] {
        let moved = transform(&phrase, &chain);
        assert_eq!(moved.len(), phrase.len());
        // A rest stays a rest, and an altered note stays altered: the chromatic
        // note that gives a tune its character survives every transformation.
        assert_eq!(
            moved.iter().filter(|event| event.step.is_none()).count(),
            1,
            "{chain:?} lost the rest"
        );
        assert_eq!(
            moved
                .iter()
                .filter_map(|event| event.step)
                .filter(|step| step.alteration != 0)
                .count(),
            1,
            "{chain:?} lost the alteration"
        );
    }

    // Retrograde twice is the tune again, and augment then diminish is too.
    let there_and_back = transform(&phrase, &[Transform::Retrograde, Transform::Retrograde]);
    assert_eq!(there_and_back, phrase);
    let stretched = transform(&phrase, &[Transform::Augment, Transform::Diminish]);
    assert_eq!(stretched, phrase);
}
