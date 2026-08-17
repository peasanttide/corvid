//! Reading a `.sf2` image, and refusing one that is not.

#![cfg(feature = "synth")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

mod common;

use corvid_music::{Bank, BankError, GeneratorKind, SampleKind};

#[test]
fn a_bank_parses_into_its_presets_instruments_and_samples() {
    let bank = Bank::parse(&common::image()).expect("the fixture is well formed");

    assert_eq!(bank.name.as_deref(), Some(common::BANK_NAME));
    assert_eq!(bank.presets.len(), 1, "the terminal record is not a preset");
    assert_eq!(bank.instruments.len(), 1);
    assert_eq!(bank.samples.len(), 1);

    let preset = bank.presets.first().expect("one preset");
    assert_eq!(preset.name, common::PRESET_NAME);
    assert_eq!(preset.bank, 0);
    assert_eq!(preset.program, 0);
    assert_eq!(preset.zones.len(), 1);
    assert_eq!(
        preset.zones[0].index(GeneratorKind::INSTRUMENT),
        Some(0),
        "the preset zone points at the instrument"
    );

    let instrument = bank.instruments.first().expect("one instrument");
    assert_eq!(instrument.zones.len(), 1);
    let zone = &instrument.zones[0];
    assert_eq!(zone.range(GeneratorKind::KEY_RANGE), Some((0, 127)));
    assert_eq!(zone.amount(GeneratorKind::SAMPLE_MODES), Some(1));
    assert!(zone.matches(60, 100));
}

#[test]
fn a_sample_carries_its_audio_and_its_loop() {
    let bank = Bank::parse(&common::image()).expect("the fixture is well formed");
    let sample = bank.samples.first().expect("one sample");

    assert_eq!(sample.pcm.len(), common::FRAMES);
    assert_eq!(sample.sample_rate, common::SAMPLE_RATE);
    assert_eq!(sample.original_key, common::ROOT_KEY);
    assert_eq!(sample.kind, SampleKind::Mono);
    assert!(sample.kind.is_playable());

    // The loop is rebased onto the sample's own frames, so nothing downstream
    // has to know the pool it was cut out of.
    assert_eq!(sample.loop_start, 8);
    assert_eq!(sample.loop_end, u32::try_from(common::FRAMES).unwrap() - 8);
    assert!(sample.pcm.iter().any(|value| *value != 0));
}

#[test]
fn a_preset_is_found_by_bank_and_program_and_falls_back() {
    let bank = Bank::parse(&common::image()).expect("the fixture is well formed");
    assert!(bank.preset(0, 0).is_some());
    // Nothing answers to bank 128 program 12, so the fallback chain ends at the
    // first preset there is rather than at silence.
    assert_eq!(
        bank.preset(128, 12).map(|preset| preset.name.as_str()),
        Some(common::PRESET_NAME)
    );
    assert!(Bank::default().preset(0, 0).is_none());
}

#[test]
fn what_is_not_a_bank_is_refused_rather_than_guessed_at() {
    assert_eq!(
        Bank::parse(b"short"),
        Err(BankError::Truncated { found: 5 })
    );
    assert_eq!(Bank::parse(b"NOPE\0\0\0\0sfbk"), Err(BankError::NotRiff));
    assert_eq!(
        Bank::parse(b"RIFF\0\0\0\0WAVE"),
        Err(BankError::NotSoundFont)
    );

    // A RIFF `sfbk` with nothing in it is a SoundFont with no hydra, and the
    // error says which chunk was wanted rather than that something went wrong.
    let mut empty = b"RIFF".to_vec();
    empty.extend_from_slice(&4u32.to_le_bytes());
    empty.extend_from_slice(b"sfbk");
    assert_eq!(
        Bank::parse(&empty),
        Err(BankError::Missing { chunk: "pdta" })
    );
}

#[test]
fn a_hydra_chunk_of_the_wrong_length_is_named_in_the_error() {
    // One byte lopped off the `phdr` chunk's payload, which no longer divides
    // by the thirty-eight-byte record. The whole array is parsed before
    // anything is built, so this is caught at the point it is read.
    let image = common::image();
    let at = find(&image, *b"phdr").expect("the fixture has a phdr chunk");
    let mut broken = image.clone();
    let length = u32::from_le_bytes([
        broken[at + 4],
        broken[at + 5],
        broken[at + 6],
        broken[at + 7],
    ]);
    broken[at + 4..at + 8].copy_from_slice(&(length - 1).to_le_bytes());

    assert!(matches!(
        Bank::parse(&broken),
        Err(BankError::RecordSize { chunk: "phdr", .. })
    ));
}

/// Where a four-byte tag starts in an image.
fn find(image: &[u8], tag: [u8; 4]) -> Option<usize> {
    image.windows(4).position(|window| window == tag)
}
