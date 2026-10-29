# `corvid_music`

A soundtrack for a [Corvid](https://github.com/peasanttide/corvid) game:
Standard MIDI Files played through a soundfont, by
[rameau](https://github.com/lizelive/rameau)'s synthesizer, to a sound card or
into a buffer.

```rust
use corvid_music::{Player, Song};

// Middle C for half a beat at 120 beats a minute, as a format-0 file.
const NOTE: &[u8] = b"MThd\0\0\0\x06\0\0\0\x01\0\x60\
    MTrk\0\0\0\x0c\0\x90\x3c\x64\x60\x80\x3c\x40\0\xff\x2f\0";

let song = Song::parse(NOTE)?;
assert_eq!(song.len(), 2);
assert_eq!(song.seconds(), 0.5);

# #[cfg(feature = "unison")]
# {
// The instruments, decoded once. `unison` is the general-MIDI bank this
// crate can embed; `Bank::parse` takes any `.sf2` or `.sf3`.
let bank = corvid_music::Bank::unison()?;
let mut player = Player::new(&bank, 48_000);
player.play(song)?;

// A tenth of a second of interleaved stereo, with the note in it.
let mut out = vec![0.0f32; 2 * 4_800];
player.fill(&mut out)?;
assert!(out.iter().any(|sample| *sample != 0.0));
assert!(player.playing());
# }
# Ok::<(), corvid_music::Error>(())
```

## The parts

A [`Song`] is a MIDI file read once: every track merged, the tempo map walked,
and each event placed in seconds. A [`Bank`] is a soundfont with its samples
decoded to PCM, which is the expensive step and is done once and shared. A
[`Player`] is the synthesizer over rameau's software mixer, playing one song and
writing interleaved stereo into whatever buffer it is handed; it is the half a
test drives, with no device and no clock but the samples it has written.

[`Music`] is a player on a worker thread feeding the default output device
through `cpal`. The worker renders a tenth of a second ahead into a queue sized
before the stream starts, and the callback only pops from it, so the callback
allocates nothing and waits for nothing -- the rule `corvid_audio` sets out. The
gain is applied in the callback, so a volume change is heard at once, while a
song asked for is heard up to that tenth of a second later. The stream lives on
the worker, so a [`Music`] may be held anywhere, unlike `corvid_audio::Audio`.

```rust,no_run
# #[cfg(feature = "unison")]
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use corvid_music::{Bank, Music, Song};

// A machine with no sound card answers `Err`: carry on without music. The
// bank is decoded on the worker, after the device answered.
let music = Music::open(Bank::unison)?;
music.set_gain(0.3);
music.play(Song::parse(&std::fs::read("theme.mid")?)?);
// `playing` goes false once the song and its tail are over: choose the next.
# Ok(()) }
# #[cfg(not(feature = "unison"))]
# fn main() {}
```

## Scope

This plays songs, one at a time, from their beginning. It does not crossfade
two songs, seek, loop a section, change tempo, or spatialize: a soundtrack is
heard at the listener, and a game that wants one song to follow another asks
for it when [`Music::playing`] goes false. It does not compose; rameau's
`rameau_compose` does, and is not linked. Nothing here is deterministic or
hashed: music is the device half of a game, like `corvid_audio`, and a run with
nobody to hear it should not open it at all.

## Licence

This crate is **AGPL-3.0-or-later**, unlike the rest of Corvid, because it links
rameau, which is. Corvid's GPL-2.0-or-later code may be combined with AGPL code
under version 3 terms, so a binary that turns this crate on is distributed under
the AGPL. The Unison bank behind the `unison` feature is CC0-1.0.

## Features

| Feature | Effect |
|---|---|
| *(none)* | [`Song`], [`Bank`], [`Player`]; [`Music::open`] answers [`Unavailable`]. |
| `unison` | [`Bank::unison`], the embedded general-MIDI bank (about 6.6 MiB). |
| `device` | [`Music`] opens the default output device through `cpal`. |

## Tests

```sh
cargo test -p corvid_music --all-features
```

`tests/player.rs` plays the bank into buffers: silence with no song, sound with
one, the song ending on time and its tail, stopping, and a song started over
another. The device path needs a sound card, so `examples/play` is the check run
by hand:

```sh
cargo run --release -p corvid_music --features device,unison --example play -- song.mid
```
