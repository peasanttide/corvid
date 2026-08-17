# `corvid_music`

Music that is composed rather than played back.

Twelve musical parameters go in and one bar of music comes out, and the bar
after it is composed once you have heard this one. There is no playlist, no stem
mixer and no crossfade between a calm loop and a combat loop: a [`Composer`]
reads the state it is given, decides what the next bar is, and writes it down.

```rust
use corvid_music::{Composer, Event, Motif, MotifId, Parameters, Role, Step};

fn tune() -> Motif {
    Motif::new(
        MotifId(1),
        vec![
            Event::note(Step::new(0), 1.0),
            Event::note(Step::new(2), 0.5),
            Event::note(Step::new(4), 0.5),
        ],
    )
}

let mut composer = Composer::new(1789, Parameters::default());
composer.motifs_mut().insert(tune());

let bar = composer.next_bar();
assert_eq!(bar.motif, Some(MotifId(1)));
assert!(bar.voice(Role::Lead).is_some());
assert!(!bar.is_silent());

// The same seed and the same parameters give the same bar, which is what makes
// anything here testable.
let mut again = Composer::new(1789, Parameters::default());
again.motifs_mut().insert(tune());
assert_eq!(again.next_bar(), bar);
```

The crate is `no_std` plus `alloc`, owns no device, and is two halves behind two
features. `compose` is the composer above. `synth` is a [`Synth`]: MIDI, a
`SoundFont` 2 [`Bank`], and a mixer that fills a buffer of samples. Each builds
without the other, and with both on there is a [`perform`] between them.

Floating point throughout, deliberately. This is a client-ring crate -- nothing
in it is hashed, nothing in it is sent, and two machines are allowed to disagree
about a picture or a sound -- so a gain can be an `f32` where a position could
not. What replaces determinism-by-arithmetic is determinism-by-seed: every
random decision a composer makes is drawn from one generator that
[`Composer::new`] sets, so a seed and a parameter set reproduce a run exactly.

## The twelve parameters

[`Parameters`] carries them. Everything but `tempo` and `voices` is a proportion
in `0.0 ..= 1.0`, and an out-of-range value clamps rather than fails, because it
arrived from arithmetic somebody else did.

| | what it moves |
|---|---|
| `tempo` | beats per minute |
| `density` | how many notes to a beat the accompaniment aims for |
| `voices` | how many pitched lines there are, `1 ..= 7` |
| `dissonance` | how far the counterpoint rules may be broken |
| `chromaticism` | how cheaply a note may leave the mode |
| `ornament` | how often the tune is decorated |
| `mode_dark` | where on the ladder from lydian to phrygian the mode sits |
| `syncopation` | how much weight lands off the beat |
| `harmonic_rate` | how willing the harmony is to move, and how long a phrase is |
| `refinement` | what the leap, spacing and metre rules are worth |
| `register` | how high the whole texture sits |
| `grit` | how much percussion, and how hard |

They are read fresh for every bar. Six of them act inside the bar they arrive in
-- density, ornament, register, grit, syncopation and dissonance -- and six need
a boundary, because they choose the mode, the metre and how many lines there
are. [`Composer::arm`] and [`Composer::interrupt`] are how a boundary is made to
arrive early.

Nothing in that list is a game concept, and that is the point. This crate knows
about tempo and dissonance; what a game maps onto them is its own business and
stays on its own side of the fence.

## What a bar is

A [`Bar`] is the whole output: a tempo, a metre, a tonic, a [`Mode`], the
[`Chord`] the accompaniment is built on, whether a [`Cadence`] landed, which
motif is being quoted, and a [`Voice`] per line holding [`Note`]s with onsets in
beats. There is no audio in it and no instrument. Which instrument plays a
[`Role`] is a data pack's decision -- a range, an affinity and a set of permitted
roles are records a game loads -- and this crate never learns one. What it
decides is that there is a line here, in this range, doing this job.

```rust
use corvid_music::{Composer, Parameters, Role};

let mut composer = Composer::new(4, Parameters { voices: 4, ..Parameters::default() });
let bar = composer.next_bar();

assert_eq!(bar.pitched(), 4);
assert_eq!(bar.voices[0].role, Role::Lead);
assert!(bar.seconds() > 0.0);
assert!(bar.onsets_per_second() > 0.0);
```

Degrees rather than pitches is why a motif survives everything done to it. A
[`Step`] says "the third degree, a semitone flat, an octave up", and what that
sounds like is decided when it is resolved through the bar's own mode -- so a
tune transposed, inverted or moved into a darker mode is still the tune, and a
chromatic alteration is still chromatic afterwards.

## The melody is never negotiated

The tune is quoted material. The chord is chosen to fit the tune and only the
accompaniment is searched, which is the reverse of the obvious direction and is
the single most important rule in the crate. A melody annealed into notes that
are in key, on chord tones and no longer the melody is the failure the rule
exists to prevent.

So the search moves inner voices between chord tones and octaves and nothing
else. Whatever it finds is in the chord by construction, and the lead is
untouched by it.

## Destruction buys permission to break the rules

Every rule that is a matter of taste is scaled by `1 - 0.7 * dissonance`:
parallels, voice crossing, spacing, leaps, doubling. Two rules are never scaled
at any value. The tune is never obscured, and no line is ever written outside
its range.

At or below [`Composer::STRICT_DISSONANCE`] the composer makes a promise rather
than an effort: the bar it hands back contains no parallel fifths or octaves at
all, counted inside the bar and across the barline. It keeps that promise by
escalating -- another chord tone, then holding the line on one key so it has
nothing to move in parallel with, then delaying its entry, then silencing it,
which is the same answer the constructive pass gives to a voice with no room
under the tune. [`parallel_perfects`] is the same function the tests check with,
so the checker and the composer cannot disagree about what the rule meant.

```rust
use corvid_music::{Composer, Parameters, parallel_perfects};

let mut composer = Composer::new(31, Parameters { voices: 5, ..Parameters::default() });
let first = composer.next_bar();
let second = composer.next_bar();

assert_eq!(parallel_perfects(&first, None), 0);
assert_eq!(parallel_perfects(&second, Some(&first)), 0);
```

## The cadence you are not allowed to have

A phrase cadences at its last bar. While the tension a caller reports is still
rising, the cadence is refused: the penultimate bar repeats under a deceptive
chord and the phrase does not end, up to [`Composer::MAX_DEFERRALS`].
Resolution lands when the tension stops climbing.

This is the most legible thing the crate does. The music will not let you go,
and then it does.

```rust
use corvid_music::{Cadence, Composer, Parameters};

let mut composer = Composer::new(8, Parameters::default());
let mut tension = 0.0f32;

for _ in 0..14 {
    tension += 0.05;
    composer.set_tension(tension);
    assert_eq!(composer.next_bar().cadence, None);
}
assert!(composer.deferrals() > 0);

// The tension stops rising, and the phrase is allowed to close.
let mut landed = None;
for _ in 0..4 {
    composer.set_tension(tension);
    if let Some(cadence) = composer.next_bar().cadence {
        landed = Some(cadence);
        break;
    }
}
assert_eq!(landed, Some(Cadence::Authentic));
```

## Motif memory

A [`Motif`] is a short idea in degree space with a [`Subject`] it is about and a
heat that says how present that subject has been. [`MotifPool::warm`] raises the
heat of everything bound to a subject, the composer cools the pool once a bar,
and a draw is weighted by what is left. So the tune that played when a subject
was last present comes back when it is present again, transformed by whatever
the parameters are by then. That costs one float per motif and it is the whole
reason a score beats a playlist.

The transformations -- [`Transform::Transpose`], [`Transform::Invert`],
[`Transform::Retrograde`], [`Transform::Augment`] and [`Transform::Diminish`] --
are applied once, deliberately, when a variation begins, and never by random
search. The tune you hear is the tune, transformed on purpose.

```rust
use corvid_music::{Event, Step, Transform, transform};

let phrase = [
    Event::note(Step::new(0), 1.0),
    Event::note(Step::new(2).altered(-1), 1.0),
    Event::note(Step::new(4), 1.0),
];

// Every transformation works in degree space, so a chromatic note stays
// chromatic through it.
let inverted = transform(&phrase, &[Transform::Invert]);
assert_eq!(inverted.len(), phrase.len());
assert!(inverted.iter().filter_map(|event| event.step).any(|step| step.alteration != 0));

// Retrograde twice is where it started.
assert_eq!(transform(&phrase, &[Transform::Retrograde, Transform::Retrograde]), phrase);
```

[`contour_similarity`] is how "recognisable" is measured: it compares the shape
of two lines rather than their pitches, so a line and the same line transposed,
an octave up, or in another mode all score `1.0`.

## Reacting inside a bar

Recomputing the parameters every bar is a bar-level instrument. Two more things
make it finer. [`Composer::arm`] queues a new phrase for the next bar, optionally
on a named motif. [`Composer::interrupt`] does not wait: it cuts the bar just
written short at a beat, drops everything that had not started, leaves what had
started ringing out with the length it was written with, and begins a new phrase.
The bar it answers with is the bar that was actually heard, and the composer
keeps that one as its history, so the next bar's voice leading joins the music
rather than the plan.

```rust
use corvid_music::{Composer, Parameters};

let mut composer = Composer::new(6, Parameters::default());
let whole = composer.next_bar();
let cut = composer.interrupt(1.0).unwrap_or_else(|| whole.clone());

assert!(cut.elided);
assert_eq!(cut.beats, 1.0);
assert!(cut.onsets() < whole.onsets());
assert!(cut.voices.iter().all(|v| v.notes.iter().all(|note| note.beat < 1.0)));
```

## The synthesizer

[`Synth`] takes [`MidiEvent`]s, either now or scheduled against its own frame
clock, and fills an interleaved stereo buffer. It owns no device: whether that
buffer reaches a sound card, a file, or a mixer that also has footsteps in it is
somebody else's decision. Every sample it writes is in `-1.0 ..= 1.0`, by a hard
clamp after the master gain -- a clip is a decision a caller can hear and act on,
and a sample outside the range is a click nobody can trace.

[`Bank::parse`] reads a `.sf2` image into presets, instruments, zones and
samples. A zone's generators are merged the specification's way -- instrument
generators replace, preset generators add -- and what comes out is a pitch, a
gain, a pan, a loop and a six-segment volume envelope per sounding layer. A
generator this crate does not act on is carried through as the number it was
rather than dropped, because a bank quietly losing the one generator that made an
instrument sound right is worse than a bank carrying one nobody reads.

With no bank, a channel plays a [`Waveform`]. That is enough to hear that the
harmony works and nowhere near enough to hear the music.

```rust
use corvid_music::{MidiEvent, Synth, Waveform};

let mut synth = Synth::new(48_000);
synth.set_waveform(0, Waveform::Sine);
synth.send(MidiEvent::NoteOn { channel: 0, key: 69, velocity: 100 });

let mut block = vec![0.0f32; 2 * 4_800];
synth.render(&mut block);

assert!(block.iter().any(|sample| sample.abs() > 0.01));
assert!(block.iter().all(|sample| sample.abs() <= 1.0));
```

[`perform`] is the seam between the halves, and it is deliberately thin: a bar is
notes with onsets in beats, a synthesizer wants messages with onsets in frames,
and that multiplication is all it does. One channel per line, percussion on nine,
and a note-off ordered before any note-on that shares its frame, so a repeated
key is not released the instant it starts.

```rust
use corvid_music::{Composer, Parameters, Synth, perform};

let mut composer = Composer::new(2026, Parameters::default());
let mut synth = Synth::new(48_000);

let bar = composer.next_bar();
synth.schedule_all(perform(&bar, 48_000, synth.clock()));

let mut block = vec![0.0f32; 2 * 4_800];
synth.render(&mut block);
assert!(block.iter().any(|sample| sample.abs() > 0.01));
```

## Feeding an audio frame instead

The `sound` feature is the other way to sound a score, for a game whose
catalogue already holds a recording per instrument. [`write_bar`] writes every
note as a cue on a bus of its own and [`music_bus`] is that bus. What it buys is
the platform's own mixer and its own voice budget; what it costs is that every
note is one recording resampled, which is audible a long way from its root key.

The score has no position. Every cue it writes sits at the listener, so it
neither pans nor occludes -- which is exactly what makes it the score rather than
a street singer, and a street singer is a source a game places itself.

## Features

All off by default, and `compose` and `synth` each build alone.

| feature | effect |
|---|---|
| `compose` | the composer: [`Composer`], [`Bar`], [`Motif`], [`Parameters`] |
| `synth` | the synthesizer: [`Synth`], [`Bank`], [`MidiEvent`], [`Waveform`] |
| `sound` | [`write_bar`] and [`music_bus`]; implies `compose` |
| `serde` | `Serialize` and `Deserialize` on the data a pack carries |
| `std` | forwards `std` to whichever of the above are on; adds no API |

`libm` is a plain dependency rather than an optional one, and it is written in
this crate's own manifest rather than the workspace's, because nothing else in
the workspace names it. The workspace does have a float crate, and it is
deliberately the wrong tool here: `corvid_float` is software floating point
chosen so a projection matrix can be a `const`, it is slower than the intrinsic
at runtime on purpose, and it has no exponential at all. A cent, an envelope
segment and a frequency ratio each need one, once per sample.

This page is the front page of a build with both halves on. A build with only one
of them gets a paragraph instead, because a front page whose examples do not
compile is worse than no front page.

## Scope

This crate composes and it sounds. It will grow more of both: more forms than the
phrase-and-cadence shape it has now, grounds and figured basses read from a pack,
fugal answer and stretto, more agrements, and the modulators, filter and low
frequency oscillators a bank describes and this synthesizer ignores.

It will not grow a device, a file system or a decoder. Opening a sound card is
`corvid_audio`'s job and reading a pack is `corvid_asset`'s. A `.sf3` -- a bank
whose samples are Ogg/Vorbis streams -- parses as far as its structure and then
has no audio, because decoding Vorbis is a codec and a codec is somebody else's
crate. There is no `.mid` reader either: this crate emits MIDI and consumes it,
and reading a standard MIDI file is a job for whatever turns a corpus into a
pack, which happens once at build time and never in a game.

It will not grow game concepts. There is no hunger here, no riot, no crowd and no
fire -- only tempo, dissonance and how present a subject is. A game maps its own
state onto the twelve parameters and warms the subjects it wants heard, and that
mapping lives in the game.

And it will not grow an instrument catalogue. Which instrument plays which
[`Role`], what its range is and what it costs are records in a data pack, and a
composer that knew about a bassoon would be a composer a level author could not
change.

## What is not built yet

Said plainly rather than left to be discovered.

The search is a bounded anneal over inner-voice placement followed by a
deterministic repair, not the full weighted rule set with a per-rule cost report
that a debug overlay would want. The cost function is there and its weights move
with the parameters, but nothing yet hands the breakdown back.

`chromaticism` relaxes what a note outside the chord costs the harmony, decides
how often a seventh is added, and makes a port-de-voix lean from a semitone below
rather than a scale step. It does not yet write chromatic passing notes or
borrowed chords.

Form selection is one shape. The proof of concept this crate is a port of scored
six dance forms against its parameters and chose between them; here the phrase
length follows `harmonic_rate` and the metre follows `refinement`, and the forms
themselves are not modelled.

A bank's modulators are read for their length and discarded, and the filter, the
low frequency oscillators and the modulation envelope are parsed and not applied,
so a bank that leans on them sounds duller here than it does elsewhere.

## Tests

```sh
cargo test -p corvid_music --all-features
```

| file | covers |
|---|---|
| `tests/compose.rs` | a seed reproducing a bar, tempo against onset rate, voice count against voice leading, the mode ladder, every note in the scale, cadence deferral, interruption |
| `tests/motif.rs` | a warmed motif recurring and recurring as itself, heat deciding a draw, a cold pool still answering, every transformation keeping its alterations |
| `tests/bank.rs` | a `.sf2` image built byte by byte and read back, and four ways of not being one |
| `tests/render.rs` | notes into samples with a bank and without, scheduling, the range promise, and a composed bar rendered |
| `tests/frame.rs` | a bar written into an audio frame, on its own bus, at the listener |
| doctests | every `rust` block on this page and in the item documentation |

That last row is not a formality. This page is the crate's front page, so every
block above is compiled and run by `cargo test`, and a claim that stops being
true stops the build.
