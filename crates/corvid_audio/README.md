# `corvid_audio`

The device half of [Corvid](https://github.com/peasanttide/corvid)'s audio: an
[`AudioFrame`]'s one-shots turned into procedurally generated waveforms and mixed
to a sound card. `corvid_sound` says what a game wants heard; this is the first
thing in the workspace that makes any of it audible.

```rust
use corvid_audio::{Catalogue, Heard, Mixer, Timbre, notes};
use corvid_sound::{AudioFrame, Cue, SoundId};
use corvid_time::Tick;

const THUD: SoundId = SoundId(2);

// What a sound is, in the absence of any recording to load.
let catalogue = Catalogue::new().with(THUD, Timbre::knock(90.0).with_decay(0.2));

// The frame a game's `hear` filled: one bounce, fired on tick 97.
let mut frame = AudioFrame::new();
let id = frame.next_id(Tick(97));
frame.cue(Cue::new(id, THUD));

// The decision half. `heard` is what keeps the ten displayed frames that can
// see tick 97 from playing the bounce ten times.
let mut heard = Heard::new(256);
let mut pending = Vec::new();
notes(&frame, &catalogue, &mut heard, &mut pending);
assert_eq!(pending.len(), 1);

// The arithmetic half, which a device drives and a test can drive instead.
let mut mixer = Mixer::new(48_000, 64);
for note in pending.drain(..) {
    mixer.start(note);
}
let mut buffer = [0.0f32; 480];
mixer.fill(&mut buffer, 1);
assert!(buffer.iter().any(|sample| *sample != 0.0));

// And the same frame again is the same bounce, not a second one.
notes(&frame, &catalogue, &mut heard, &mut pending);
assert!(pending.is_empty());
```

## What this plays, and what it does not

Be exact about the scope, because most of an audio backend is not here.

It plays **cues**: the one-shots a frame fires. Each becomes one voice, at the
cue's gain multiplied through its bus chain and the listener's gain, at a
frequency the cue's pitch multiplies.

It does not play **sources**. A [`Source`] is a voice a backend holds open
across frames — a torch burning, an engine running — and playing one needs a
loop, an envelope, and a rule for what happens on the frame it stops being in
the list. None of those is here, so a frame's sources are carried past this
crate untouched.

It does not **spatialize**. Every voice goes to every channel at the same
amplitude, so a cue thirty metres to the left sounds exactly like one at the
listener's feet. `src/extract.rs` has a test that says so — a frame with a
distant cue and a frame with the same cue at the origin produce the same notes —
and that test is meant to fail on the day this stops being true. What would
close it is a panner and a distance curve, and past that the platform's own
HRTF. The frame's positions are `FinePoint` offsets in the listener's own frame,
which is already the shape a panner wants.

It does not **limit**. Voices are summed and clipped hard at full scale, which
distorts when enough loud cues land together.

There is **no reference mixer and no WAV golden**. What is here is `f32`
arithmetic on a device's own thread, which is the production path rather than a
deterministic one, so its output is not something to freeze. Frames are
golden-testable in `corvid_sound`, and that is the whole of what is frozen.

## There are no recordings, so a sound is described

A [`SoundId`] names a recording in a catalogue, and there are no recordings here
to load. So a [`Catalogue`] here maps an identifier to a [`Timbre`] —
a frequency, a decay, an attack, and how much of the octave above is mixed in —
and a voice inside the [`Mixer`] is the oscillator that plays it. Four numbers
make a thud, a click, a chime and a thump, and nothing beyond that.

An identifier nobody described is **not silent**. It is played as a knock at a
pitch derived from its number, over two octaves from the A below middle C, so
that a cue the game fired is a sound the game hears. Silence would make a
missing catalogue entry and a cue that was never fired look identical, and
telling those two apart is most of what a person debugging audio is doing. That
derivation is a placeholder in the strong sense — it has no idea what any sound
means, and which note one lands on is an accident of the number the game gave
it.

## The cue problem, and which parts of it this answers

`corvid_sound` sets this out at length and it is the reason a [`Cue`] carries a
[`CueId`] at all. A cue is fired by the simulation and read once per *displayed*
frame, so a fifteen-hertz tick is read nine or ten times over; and rollback
re-simulates ticks that have already been read, while sound does not rewind.

So there are four cases and [`Heard`] takes a position on each.

**Read again.** The same identity in the next frame is the same sound and is not
started twice. That is the case that would otherwise play every bounce ten
times, and it is what the record exists for.

**Un-fired.** An identity that was played and then disappeared from the frame
names a sound that should not have been heard. This backend **lets it ring
out**. The sounds it makes are short percussive knocks, where cutting a voice
mid-ring is a click and letting it finish is a sound nobody notices was wrong.
That reasoning does not survive a sound with a long tail: a backend playing
recorded music would want to duck it, and ducking is a per-sound decision that
needs a catalogue this crate does not have.

**Re-fired with a different payload.** An identity that comes back having
already been started is not restarted, and the voice already playing keeps the
gain and pitch it started with. Retuning a percussive one-shot part way through
its decay is a click, and there is nothing here to retune anyway.

**Never seen, or forgotten.** [`Heard`] remembers a fixed number of identities —
256 in the device backend, which at fifteen hertz and a handful of cues a tick
is a couple of seconds. An identity older than that is treated as new and played
again. That is the cost of a bounded memory rather than a bug, and it is
asserted in `src/heard.rs` rather than hoped for.

### What a mixer still has to decide, and this one does not

**How old is too old.** A frame extracted after a stall carries every cue of the
tick it was extracted from, and a backend that started all of them at once fires
a burst of sounds for events the player has walked past. Deciding needs a tick
and a rate to compare against, and an `AudioFrame` carries neither.

**Whether a cue is worth a voice.** With every voice busy this steals the
quietest, on the grounds that the quietest is the closest to finished. A mixer
with a mix to protect would rather drop the new sound, or duck a bus, or decide
by category — and a category is a thing a catalogue knows.

**What a bus cycle means.** `corvid_sound` accepts a bus that names itself as
its parent and says so. This walks the chain as many steps as there are buses
and stops, which is finite and is not an answer; a backend that resolved the
graph properly would reject the frame or break the cycle deliberately.

**What a missing bus means.** A cue naming a bus the frame does not carry is
given no trim at all rather than silence, because a game that lost every sound
over a missing list entry is much harder to diagnose than one that lost a volume
trim. That is a reading, not a rule.

**Everything a source needs**, as above.

## The callback may not allocate, wait, or panic

The audio callback runs on a thread the operating system schedules against a
deadline of a few milliseconds. Allocating may take a global lock; waiting may
miss the deadline; panicking unwinds through a foreign stack frame. So the
interesting thing to write down is not that it does none of the three, but how
it is arranged so that it cannot.

**Nothing in it allocates.** Everything it touches is sized before the stream
starts. The mixer's voice pool is a boxed slice built by `Mixer::new`; the queue
between the two threads is a `VecDeque` whose capacity is reserved in
`Audio::open`, and the callback only ever pops from it. Samples are mixed one at
a time straight into the device's own buffer, so there is no intermediate to
size — which is why `Mixer` exposes `next_sample` and not only `fill`.

**Nothing in it waits.** It takes the queue with `try_lock` and carries on
without it when the game's thread is holding it, so a note can be one device
period late — a few milliseconds — and a callback is never blocked by a game
thread that has been preempted. The other side is held only for as long as
pushing a handful of values takes, so the game's thread waits for microseconds
and it is the side that may.

**Nothing in it panics.** The workspace denies `unwrap`, `expect`, `panic` and
`unreachable`; beyond what the lints see, the callback indexes nothing, divides
by nothing, and does all its arithmetic in `f32`, so there is no overflow to
differ between a debug build and a release one. Every value that reaches the
oscillator is made finite and in range on the way in — `src/voice.rs` has a test
that feeds it a zero sample rate, an infinite frequency and a `NaN` attack and
asserts that a thousand samples come out finite and inside full scale.

None of that is a proof, and it is not written as one. It is a set of
arrangements that can each be checked by reading the code they name.

## Features

| Feature | Effect |
|---|---|
| *(none)* | The mixer, the voices, the catalogue and the cue bookkeeping. No device. |
| `device` | `Audio`, which opens the platform's default output through `cpal`, and `tracing`, which is what it reports a broken stream through. |

The split is deliberate rather than tidy. The part of a backend that can be
wrong in a way a person notices is the arithmetic — a gain applied twice, a cue
played ten times, a voice that never frees its slot — and a test for any of that
should not need a machine with speakers. So the whole decision half is a plain
function, `notes`, and the whole sample half is a plain type, `Mixer`, and both
are tested on a machine with no sound card. `device` adds the thread and the
stream and the two dependencies they need — `cpal` for the device and `tracing`
for the three things only a device can report — and nothing else. Everything
that can go wrong in the featureless half is a value a caller can read, so a
build without the feature links neither.

`Audio` is deliberately neither `Send` nor `Sync`, and that is arranged rather
than inherited: a `cpal::Stream` is tied to the thread that built it on some
platforms and not on others, so the type carries a `PhantomData<*const ()>` to
make the compiler refuse the move everywhere. A `compile_fail` doctest on the
type is the check, with its counterpart in `tests/mixer.rs` asserting that the
half with no device in it does cross threads.

A game that wants to be heard opens one device and hands it a frame per
displayed frame:

```rust,no_run
# #[cfg(feature = "device")]
# fn main() -> Result<(), corvid_audio::Unavailable> {
use corvid_audio::{Audio, Catalogue, Timbre};
use corvid_sound::{AudioFrame, SoundId};

const THUD: SoundId = SoundId(2);

// A machine with no sound card answers `Err`. That is a thing to carry on
// without, not a thing to stop for.
let mut audio = Audio::open(Catalogue::new().with(THUD, Timbre::knock(90.0)))?;

let frame = AudioFrame::new();
audio.hear(&frame);
# Ok(())
# }
# #[cfg(not(feature = "device"))]
# fn main() {}
```

## Tests

```sh
cargo test -p corvid_audio --all-features
```

| File | Covers |
|---|---|
| `src/voice.rs` | The envelope's start and end, gain, timbre against volume, and a device reporting nonsense |
| `src/heard.rs` | One cue read ten times, two cues on one tick, a rollback, and the ring's own limit |
| `src/catalogue.rs` | A described sound, an undescribed one, and the derivation's range |
| `src/extract.rs` | Which gains multiply, a missing bus, a bus cycle, pitch, and that a position changes nothing |
| `tests/mixer.rs` | Voice stealing, the clip, that a mixer with nothing playing writes silence rather than the last buffer |
| doctests | Every `rust` block in this file |

That last row is not a formality: this README is the crate's front page, so every
`rust` block above is compiled and run by `cargo test`, and a claim that stops
being true stops the build.

What no test here covers is the device path, because opening one needs a sound
card and a test that skipped when there was none would be a test that never ran
anywhere. `examples/knock` is that check, run by hand:

```sh
cargo run --release -p corvid_audio --features device --example knock
```

On the machine this was written on it reports `48000 Hz, 2 channels` and, after
six seconds, `the device took every note it was given` — so the stream opened,
the callback was called, and every note crossed between the threads. That
machine has no sound card; the default ALSA device routes to a sound server, which
consumed the samples without anything being audible. **Whether it sounds right
has not been checked here**, and running the same command on a machine with
speakers is what would check it: four knocks a second for six seconds, each a
distinct sound rather than a click or a buzz, every fourth one brighter, with no
repeats. A cue read by four displayed frames and played four times is the
failure most likely to show, and it sounds like a flam rather than a knock.

[`AudioFrame`]: corvid_sound::AudioFrame
[`Source`]: corvid_sound::Source
[`Cue`]: corvid_sound::Cue
[`CueId`]: corvid_sound::CueId
[`SoundId`]: corvid_sound::SoundId
