# `corvid_sound`

What a [Corvid](https://github.com/peasanttide/corvid) game wants heard, as
data. An [`AudioFrame`] is a listener, a list of playing [`Source`]s, a list of
one-shot [`Cue`]s, and a list of [`Bus`]es — and no device, no voices, no sample
buffer, and no mixer. Nothing in this crate makes a sound.

That is the whole design rather than a limitation. The frame is the artefact
goldens compare. Nothing in it is a float, which removes the usual reason two
machines that computed the same thing disagree about the bytes, and a capture
taken today diffs against one recorded by last month's build. Turning a frame into samples is a backend — a device-native
spatializer in production, where the platform's own HRTF beats anything this
workspace would write, and a small reference mixer in a headless run, which
writes bit-identical WAV. The honest consequence is that a WAV golden validates
the frame and the reference mixer and never the production audio path.

What is here is the frame a runtime, a capture and a backend are each handed,
and the parts of it a mixer's behaviour is built on. `Auralizer::hear` is
what fills one, `corvid_app` keeps one for the life of a run and writes it into
every captured frame, and `corvid_audio` is the mixer that turns one into
samples.

## Every position in a frame is relative to the listener

This is the one thing to know before writing anything that fills a frame.
[`Listener::pose`] is in world space; every [`Source`] and [`Cue`] position is an
**offset in the listener's own frame**, in the workspace's right-handed
**+X right, +Y forward, +Z up** convention, and neither type carries a
world-space position at all. So an extractor — `Auralizer::hear` — is
handed the ears *and* the sounds, and does the subtraction and the rotation
itself. A `hear` written as though a source carried a world position compiles
against these types and is wrong everywhere except the origin, which is exactly
where it would be tested.

The reason is width. A [`FinePoint`] is 32 bits an axis and reaches ±32.7 km at
15.26 µm, far past anything audible; an absolute position on an earth-scale world
needs the 64-bit [`GlobalFinePoint`](corvid_vector::GlobalFinePoint) the
listener's pose carries. It also means a frame recorded a hundred kilometres from
the origin is byte-identical to one recorded at it, so a capture does not get
noisier the further a session wanders. This crate does no arithmetic on positions
at all.

```rust
use corvid_fixed::{Factor16, I16F16};
use corvid_sound::{AudioFrame, Bus, BusId, Cue, Listener, SoundId, Source, SourceId};
use corvid_time::Tick;
use corvid_vector::FinePoint;

const THUD: SoundId = SoundId(1);
const TORCH: SoundId = SoundId(2);
const EFFECTS: BusId = BusId(1);

// A runtime is meant to own one frame and refill it once per displayed frame.
let mut frame = AudioFrame::new();

frame.listen(Listener::default());
frame.bus(Bus::new(EFFECTS).under(BusId::MASTER).with_gain(Factor16::from_f64(0.6)));

// A torch that has been burning for a while: a voice a backend would keep
// open, named by a `SourceId` so it is not restarted every time it moves.
frame.source(
    Source::new(SourceId(7), TORCH)
        .at(FinePoint::new(I16F16::from_f64(2.0), I16F16::from_f64(3.0), I16F16::ZERO))
        .on(EFFECTS),
);

// A bounce that happened on tick 97: an event, named by the tick it fired on.
let id = frame.next_id(Tick(97));
frame.cue(Cue::new(id, THUD).at(FinePoint::ZERO).with_gain(Factor16::from_f64(0.9)));

assert_eq!(id.to_string(), "97#0");
assert_eq!(frame.cues.len(), 1);
```

## Nothing here is a float

| Quantity | Type | Why |
|---|---|---|
| Listener pose | [`FineTransform`] | a position and a packed rotation, in the eye's own space |
| Source and cue positions | [`FinePoint`] | ±32.7 km at 15.26 µm, as an offset from the listener |
| Gains and occlusion | [`Factor16`] | `0.0 ..= 1.0`, with `65535` denoting exactly `1.0` |
| Pitch | [`I8F8`] | ±128 playback rates at `1/256`, with [`I8F8::ONE`] the recorded rate |
| Tick a cue fired on | [`Tick`] | an index into the session, not a moment |
| Identifiers | `u16` and `u32` newtypes | numbers a catalogue answers to |

An `f32` gain would be three implementations of rounding on three
architectures, and this frame is compared byte for byte. The conversions exist
— [`Factor16::from_f64`] and friends — and they belong at the edge where a
number gets *in*, not in the arithmetic that produced it.

Narrowing a world-space difference into a [`FinePoint`] is the extractor's job,
for the reason the section above gives.

## The cue problem

A [`Cue`] is a one-shot fired by the simulation, and it is the only genuinely
awkward type here. The awkwardness is rollback.

The simulation is authoritative and re-runnable. When a corrected action for
tick 95 arrives late, a runtime rewinds and re-simulates 95 onwards, and the
states it produces the second time need not match the first — that is the point
of doing it. Sound does not rewind. The thud from tick 97 has already left the
speaker by the time the correction lands. So a rollback can **un-fire a cue that
has already been played**, and can **re-fire one that was already played**.

A mixer holding a list of voices it has started therefore has to tell those two
cases apart from a third that looks identical from outside: two genuinely
different cues carrying the same sound at the same place. Nothing about the
payload can settle it, because the payload moves for reasons that have nothing
to do with the simulation. A client extracts a frame once per *displayed*
frame, so a fifteen-hertz tick will be extracted from nine or ten times, and
between two of those the listener has moved and the same cue's listener-relative
position is a different number.

So a cue carries a [`CueId`], which is the tick it fired on and its place among
that tick's cues, and which is deliberately **disjoint from the payload**:

```rust
use corvid_fixed::Factor16;
use corvid_sound::{AudioFrame, Cue, CueId, SoundId};
use corvid_time::Tick;

const THUD: SoundId = SoundId(1);

// The same tick's bounce, extracted at two display frames. The listener has
// moved between them, so the cue is quieter the second time — and it is the
// same cue, and its identity says so.
let early = Cue::new(CueId::new(Tick(97), 0), THUD).with_gain(Factor16::from_f64(0.9));
let late = Cue::new(CueId::new(Tick(97), 0), THUD).with_gain(Factor16::from_f64(0.4));
assert_eq!(early.id, late.id);
assert_ne!(early, late);

// A second bounce on the same tick is a second cue, whatever it sounds like.
let sibling = Cue::new(CueId::new(Tick(97), 1), THUD).with_gain(Factor16::from_f64(0.9));
assert_ne!(early.id, sibling.id);
assert_eq!(early.gain, sibling.gain);

// And a bounce on the next tick is a third.
let next = Cue::new(CueId::new(Tick(98), 0), THUD);
assert_ne!(early.id, next.id);
```

Serials are assigned by [`AudioFrame::next_id`], which reads the frame rather
than a counter of its own — so the numbering is reproducible from a serialized
frame alone, and a tool that loads a capture and appends to it gets the answer
the extractor got.

### What a mixer still has to decide

Nothing in this crate mixes, so nothing here shows that this identity is
*sufficient* for a mixer. What `tests/cue.rs` shows is narrower and is all that is
claimed: an identity is stable across two observations of one cue, distinct
across ticks, distinct across serials, and unmoved by every payload field. Three
decisions are left open, and a mixer has to make all three.

An identity that **disappears** after a rollback names a sound that was played
and should not have been. Cutting it is abrupt, letting it ring out is a lie,
and ducking it is a compromise; which is right depends on the sound.

An identity that **reappears**, already started, must not be started twice — but
if the re-simulation produced a different payload under the same identity, the
mixer is holding a voice playing the wrong thing. Detecting that is a
comparison, since `Cue`'s `PartialEq` covers the payload as well as the
identity. Deciding between restarting, retuning and ignoring is not.

And a cue whose identity a mixer has never seen may be new, or may be one it
finished playing and forgot. Nothing in the frame says which. How long a mixer
remembers is a memory budget, and this crate does not set it.

## What this crate does not check

Every one of these is an obligation on whoever builds the frame, stated as such
because none of them is enforced here.

The extractor must emit its cues in the same order every time it is run over the
same ticks, because a serial is a position in that order. Iterating a `BTreeMap`
keyed by an entity identifier keeps this; iterating a `HashMap` does not.

[`AudioFrame::cue`] accepts any [`CueId`], including one already in the frame.
[`next_id`](AudioFrame::next_id) is not a reservation either: two calls with no
push between them return the same identity.

A [`Bus`] may name a `parent` that is not in the frame, or itself, or a cycle;
a [`Source`] or a [`Cue`] may name a `bus` that is not there. This crate stores
the graph and never walks it, so all four are carried faithfully to a backend
that has to decide what they mean.

A [`SourceId`] is supposed to be stable while a sound plays and unique within a
frame. Nothing here checks either, and getting the first wrong restarts a voice
every frame while getting the second wrong merges two.

Whether a [`SoundId`] names anything is a question for a catalogue, and there is
no catalogue here.

## Features

All off by default, and each builds alone.

| Feature | Effect |
|---|---|
| `serde` | `Serialize`/`Deserialize` on every type, forwarded to the positions and gains |
| `std` | Forwards `std` to whichever of the above are enabled; adds no API |

`corvid_time`'s own `std` is deliberately not forwarded. It is the one
dependency whose `std` feature adds a type, and the type is `Wall`, a clock —
and Cargo unifies features across a build, so forwarding it would hand a wall
clock to everything that draws an audio frame as a side effect of something else
turning `std` on. Nothing here asks what time it is; it is told which [`Tick`] a
cue fired on.

## Two wire formats, neither of them visible to a round trip

A capture is a serialized frame and a hash trace is a digest, so this crate has
two encodings. The serialized bytes come from the derived `serde`
implementations; the digest comes from the derived `Hash` implementations under
`corvid_hash`'s hasher. Both are a function of the field declaration order, the
field count and the width of every integer, and neither states that intention
anywhere.

Neither is type-checked. Exchanging two fields of the same type compiles; adding
a field compiles; widening an identifier compiles. And neither is visible to a
serialize-then-deserialize test, because the writer and the reader move together
— a reordered frame is read back into its new order and every assertion holds
while every capture recorded yesterday now means something else.

Nor do the two cover each other, and neither of them covers a *name*.
`corvid_wire` writes a varint and no field names at all, so an identifier
widened from `u16` to `u32` writes the same bytes for the same small number and
a field renamed writes the same bytes for the same value. The digest sees the
first, because `corvid_hash` absorbs an integer as its declared bytes and injects
the count. Nothing sees the second.

So `tests/golden.rs` freezes **three** tables, as literals, over one set of
fixtures: the exact bytes, the exact digests, and what a self-describing format
writes — which is where every field name and every variant name is recorded. The
byte rows are checked in both directions — today's encoder must produce them, and
they must still decode back into the value they were recorded for, which is the
property a capture written by an older build actually needs. Changing a row in
any of the three is a wire-format break that wants a version bump, not a
regenerated table.

All three were checked by breaking them, and every break was invisible to every
round trip. Exchanging `Source`'s `sound` and `bus` — a `SoundId` and a `BusId`,
which reorder without a murmur — moves all three and leaves every round trip
alone. Adding a field to `Source` and renumbering the two variants behind a bus's
`parent` do the same. Widening `CueId`'s serial to a `u32` moves the digests and
nothing else. Renaming a field moves the text table and nothing else, which is
the one change only that table can see.

## Tests

```sh
cargo test -p corvid_sound --all-features
```

| File | Covers |
|---|---|
| `tests/frame.rs` | Building, clearing, capacity retention, `is_empty`, serial assignment |
| `tests/cue.rs` | The identity scheme: one cue observed twice, two cues on one tick, two ticks, the ceiling |
| `tests/determinism.rs` | Round trip by value and by digest, and what the two encodings separate |
| `tests/golden.rs` | The frozen encodings: the exact bytes, the exact digests, and the recorded names |
| doctests | Every `rust` block in this file and in the type documentation |

That last row is not a formality: this README is the crate's front page, so
every `rust` block above is compiled and run by `cargo test`, and a claim that
stops being true stops the build.

[`FinePoint`]: corvid_vector::FinePoint
[`FineTransform`]: corvid_transform::FineTransform
[`Factor16`]: corvid_fixed::Factor16
[`Factor16::from_f64`]: corvid_fixed::Factor16::from_f64
[`I8F8`]: corvid_fixed::I8F8
[`I8F8::ONE`]: corvid_fixed::I8F8::ONE
[`Tick`]: corvid_time::Tick
