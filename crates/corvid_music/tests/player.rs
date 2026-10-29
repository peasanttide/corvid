//! What a player writes: silence, a song, its end, a stop, and a song
//! started over another. Each is the half of the music a device drives,
//! checked without one.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::float_cmp,
    reason = "silence is exactly zero, and a song's length is a sum of exact binary fractions"
)]

use corvid_music::{Error, Song};

/// Middle C for half a beat at 120 beats a minute: a format-0 file of one
/// note on and one note off, 96 ticks apart at 96 ticks a beat.
const NOTE: &[u8] = b"MThd\0\0\0\x06\0\0\0\x01\0\x60\
    MTrk\0\0\0\x0c\0\x90\x3c\x64\x60\x80\x3c\x40\0\xff\x2f\0";

#[test]
fn a_file_is_read_into_seconds() {
    let song = Song::parse(NOTE).unwrap();
    assert_eq!(song.len(), 2, "the note on and the note off");
    assert_eq!(
        song.seconds(),
        0.5,
        "96 ticks at 96 a beat and 120 a minute"
    );
}

#[test]
fn what_is_not_a_midi_file_is_refused() {
    assert!(matches!(Song::parse(b"MThd"), Err(Error::Midi(_))));
    assert!(matches!(Song::parse(b"not music"), Err(Error::Midi(_))));
}

#[test]
fn a_build_without_a_device_opens_none() {
    #[cfg(not(feature = "device"))]
    assert!(corvid_music::Music::open(|| Err(Error::SoundFont("never decoded".into()))).is_err());
}

#[cfg(feature = "unison")]
mod with_the_bank {
    use std::sync::LazyLock;

    use corvid_music::{Bank, Player, Song};

    use super::NOTE;

    /// The rate every fixture plays at.
    const RATE: u32 = 48_000;

    /// The bank, decoded once for every test here.
    static BANK: LazyLock<Bank> = LazyLock::new(|| Bank::unison().unwrap());

    /// The loudest sample of `ms` milliseconds of the player's output.
    fn loudest(player: &mut Player, ms: u32) -> f32 {
        let frames = RATE / 1000 * ms;
        let mut out = vec![0.0f32; 2 * usize::try_from(frames).unwrap()];
        player.fill(&mut out).unwrap();
        out.iter()
            .fold(0.0f32, |most, sample| most.max(sample.abs()))
    }

    #[test]
    fn the_bank_has_the_general_midi_presets() {
        assert!(BANK.presets() >= 128, "a general-MIDI bank");
    }

    #[test]
    fn nothing_playing_is_silence() {
        let mut player = Player::new(&BANK, RATE);
        assert!(!player.playing());
        assert_eq!(loudest(&mut player, 100), 0.0);
    }

    #[test]
    fn a_song_sounds_and_ends_after_its_tail() {
        let mut player = Player::new(&BANK, RATE);
        player.play(Song::parse(NOTE).unwrap()).unwrap();
        assert!(loudest(&mut player, 250) > 0.01, "the note sounds");
        assert!(player.playing());
        // The note is off at half a second; the tail holds it as playing
        // for two more, so a next song does not cut the release short.
        let _ = loudest(&mut player, 2000);
        assert!(player.playing(), "still in its tail");
        let _ = loudest(&mut player, 500);
        assert!(!player.playing(), "over");
        assert_eq!(loudest(&mut player, 100), 0.0, "and silent");
    }

    #[test]
    fn a_stop_releases_the_song() {
        let mut player = Player::new(&BANK, RATE);
        player.play(Song::parse(NOTE).unwrap()).unwrap();
        let _ = loudest(&mut player, 100);
        player.stop().unwrap();
        assert!(!player.playing());
        let _ = loudest(&mut player, 1000);
        assert_eq!(loudest(&mut player, 100), 0.0, "released to silence");
    }

    #[test]
    fn a_song_started_over_another_plays_from_its_beginning() {
        let mut player = Player::new(&BANK, RATE);
        player.play(Song::parse(NOTE).unwrap()).unwrap();
        let _ = loudest(&mut player, 2000);
        player.play(Song::parse(NOTE).unwrap()).unwrap();
        assert!(loudest(&mut player, 250) > 0.01, "the second note sounds");
        let _ = loudest(&mut player, 2000);
        assert!(
            player.playing(),
            "timed from its own start, not the first's"
        );
    }
}
