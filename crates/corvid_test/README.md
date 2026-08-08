# `corvid_test`

The four assertions a deterministic game is worth testing with, and the
comparison that freezes what one produced.

A determinism check is not something each game should reinvent. Written out by
hand it is three hundred lines of capture-walking, hex-decoding and
golden-diffing, and every Corvid game would copy the same three hundred. Here it
is four calls, and `examples/headless` makes all four.

```rust
# use std::sync::Arc;
#
# use corvid_app::{App, Error};
# use corvid_behavior::{Command, Level, Player, ProfileId, State, Time};
# use corvid_files::{Malformed, Source};
# use corvid_control::Controller;
# use corvid_input::Input;
# use corvid_replay::{Opening, Profile, Schema, Seed};
# use corvid_time::Tick;
# use serde::{Deserialize, Serialize};
#
# #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
# struct Cliff;
# impl Level for Cliff {
#     type Reference = String;
#     fn load(_: &String, _: &dyn Source) -> Result<Self, Malformed> { Ok(Self) }
# }
#
# #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
# struct Climb { metres: i64 }
#
# #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
# enum Effort { #[default] Rest, Up }
#
# impl State for Climb {
#     const NAME: &'static str = "climb";
#     type Level = Cliff;
#     type Rules = ();
#     type Action = Effort;
#     fn tick(
#         self,
#         _level: &Cliff,
#         players: &[Player<'_, Effort>],
#         _rules: &(),
#         _command: &mut impl Command<Reference = String>,
#     ) -> Self {
#         let climbed = players
#             .iter()
#             .filter(|player| matches!(player.action, Effort::Up))
#             .count();
#         Self { metres: self.metres + i64::try_from(climbed).unwrap_or(0) }
#     }
# }
#
# #[derive(Clone, Copy, Debug, Default)]
# struct Legs;
# impl Controller<Climb> for Legs {
#     type Config = ();
#     const SETS: &'static [corvid_input::SetDescriptor] = &[];
#     fn new((): ()) -> Self { Self }
#     fn configure(&mut self, (): ()) {}
#     fn action(&self, _: &Climb, _: &Input, _: Time) -> Effort { Effort::Up }
#     fn update(
#         &mut self,
#         _: &Climb,
#         _: &Input,
#         _: Option<corvid_behavior::Loading<'_, String>>,
#         _: Time,
#         _: core::time::Duration,
#     ) {}
#     fn look(&self) -> corvid_camera::Camera { corvid_camera::Camera::default() }
# }
#
# fn opening() -> Opening<Climb> {
#     Opening {
#         level: "cliff".to_owned(),
#         content: Arc::new(Cliff),
#         rules: Arc::new(()),
#         roster: vec![Profile { account: ProfileId(1), joined: Tick::ZERO, left: None }],
#         seed: Seed(1),
#         first: Tick::ZERO,
#         origin: None,
#         schema: Schema::new("climb").field("State.metres", "i64").digest(),
#     }
# }
#
# fn main() -> Result<(), Box<dyn std::error::Error>> {
// A climber who gains a metre per tick. `State` and `Controller` for `Climb`
// are elided here; there is no renderer and no ear, because `App` defaults
// both to `()` and a game that draws nothing writes no line at all.
corvid_test::is_reproducible::<Climb, Legs>(&opening(), &(), &Input::new(&[]), 100)?;

let run = App::<Climb, Legs>::new()
    .headless()
    .opening(opening())
    .until(|climb: &Climb, _at| climb.metres >= 100)
    .run()?;

corvid_test::replays_to_itself(&run)?;
# Ok(())
# }
```

## The four

| | The claim |
|---|---|
| [`is_reproducible`] | The same opening, played twice, is the same game. |
| [`replays_to_itself`] | A run written down and read back replays into the states it recorded. |
| [`matches_goldens`] | This build computes what the last one did. |

Each bounds on the game and never on a configuration of it. [`is_reproducible`]
asks for `Present`, which is the whole client-local half — `Present` is built on
`Render`, so one bound says the runtime may call `intend`, `look`, `hear`,
`setup` and `draw`, where this crate used to need a second bound written beside
the first to say it. The two scratch-and-replay checks ask for `Simulate` alone,
and [`matches_goldens`] is handed two directories and asks for nothing.

The first three fail naming the **first tick** that differs and what differs
there. Everything after a divergence diverges too, because a simulation is a
chain, so the only tick with any information in it is the first — and an
assertion that says "the traces differ" costs whoever reads it the whole
debugging session.

They answer with a [`Diverged`], which carries that tick, the last tick the two
still agreed about, and a [`What`]: two digests, two actions and the seat that
submitted them, two reaches, two requests, or the
[`Diverged`](crate::Diverged) the reproducibility check reported. There is
no "something differs" case.

[`images_agree`] names a pixel, a channel difference and a count, and it is the
one comparison here with a tolerance argument — a frame is what a driver
rasterised rather than what the simulation computed, and [`Tolerance`] is where
that costs something. Its exact-match arm belongs to a machine that knows which
adapter it has; the bit-exact golden in a capture is the hash trace, and that one
is compared on every target.

[`matches_goldens`] names a file and a byte offset instead, and cannot name a
tick: what it compares is a directory of frozen bytes against a directory a run
wrote, and the only thing it knows about any of those files is the path the
capture calls it by. An `audio/42` is a tick and a `session` is a whole one, so
there is no tick to report for half of what it looks at. What it says is which
file moved, how many bytes each side holds and where they first differ — and
[`How::Absent`] for a golden the capture has no file for at all.

## This is a dependency, not a dev-dependency

The stand-ins in this workspace are public API rather than `#[cfg(test)]` hacks,
and this crate is one of them. A downstream game builds its own golden tests out
of it: the fixture it points [`is_reproducible`] at is its own opening, the
capture it freezes is its own, and none of that is expressible through something
that exists only inside this repository's test profile. A dev-dependency cannot
be re-exported and a dependency can, which is what settles it.

Nothing here is `#[cfg(test)]`, and nothing here is free: a crate that depends on
this one links `corvid_app`, and through it a graphics stack, whether or not the
check ever opens an adapter. A shipping build should not name it.

## What a check establishes, and what it does not

Every function's own documentation is exact about its limits. The three that are
worth knowing before reading any of them:

**Two runs in one process cannot see a constant.** A `static AtomicU64` set at
startup, a value derived from the core count, a lazily initialised table: both
runs read the same number, and there is nothing to compare. What
[`is_reproducible`] catches is a global that *moves* between runs. What catches a
constant is two peers that are genuinely two processes comparing digests, which
nothing in this workspace does.

**Two runs on one machine share a target.** Comparing the recorded digests across
architectures is a job for a CI matrix; these functions compare a build against
itself.

**A check runs the session it is handed.** A leak that needs a joining player, a
second level or a particular set of rules is invisible in a session with none of
them. `scratch_is_a_memo_throughout` is the one that most rewards being pointed
at several: it is a falsifier at each scratch value the session reached, and
passing at ninety-nine of them says nothing about the hundredth.

**A check compares the whole run, so it keeps the whole run.**
[`is_reproducible`] asks its two runs for [`Retention::Everything`] rather than
letting them take the runtime's default, which is a *window* of recent history.
Two runs that had each let go of their first thousand ticks would be compared
over what they still held, and a divergence older than the window would be
compared against nothing at all and reported as agreement. The cost is that a
check of a long run holds a row of actions and a digest per tick, twice —
`ticks` is the dial.

## Goldens

A capture holds a file per tick — two, with a device drawing — and nobody wants
to freeze two hundred of them. So the goldens directory is the frozen set for
the byte-compared half: every `.hex` file under it
names a capture file by the same relative path with the extension removed, and
that file has to be there and hold those bytes. A capture file with no golden is
not frozen and is not compared.

Freezing one more file is therefore creating one more file — `touch
goldens/audio/17.hex`, then bless. Blessing never decides on its own what to
freeze, because "the capture grew a file" and "somebody meant to freeze it" are
not the same event. A goldens directory with nothing in it is
[`Mismatch::Unfrozen`] and never a pass: a comparison with nothing in it is the
failure a golden test exists to avoid.

Goldens are hex text wrapped at thirty-two bytes to the line, because a golden is
reviewed as well as compared, and `binary files differ` is not a review.

### `CORVID_BLESS` is the only sanctioned way to change one

With [`BLESS`] set to anything non-empty, every golden that no longer says what
the capture holds is rewritten from it, and **the call still fails**, naming
every file it rewrote at once. Both halves are deliberate: a blessing run that
went green would tell nobody what it had changed, and a CI job with the variable
set by accident would go green forever. Running it again is what shows it
passing.

What blessing does not do is decide whether the change was meant. A moved golden
is one of two things and the program cannot tell them apart: the game's
arithmetic changed, or the capture format did. The first changes what every
recorded session of that game replays to; the second changes what every capture
ever recorded means. Both are one command, and neither should be.

A golden edited by hand is a golden that says what somebody expected rather than
what the runtime produced, which is the single thing a frozen capture is for.

### The goldens say which build blessed them

A `flavour` file beside them holds [`corvid_app::flavour`], and a build whose own
flavour is not the one written there refuses to compare. `corvid_app`'s `dev`
feature discards a tick's `Scratch` on a schedule that is part of the session, so
a `dev` build and a plain one are *specified* to compute different states for a
game that reads its scratch. Comparing one's goldens against the other moves
every golden at once — which is precisely what a capture-format change looks
like, so the report would be true and would send a reader to the wrong place.

That marker is the one thing in a goldens directory a person edits by hand.
Blessing writes it when it is missing, which is the first blessing of a new
directory, and never changes one that is there: rewriting a whole frozen set from
the other build is the mistake this exists to prevent and should not be one
command away. Moving a set from one build to the other is editing one line and
then blessing, which is two steps and a diff somebody can review.

[`corvid_app::flavour`]: corvid_app::flavour

## What is not here

**`Policy`, and the agent players built on it.** A trait whose `act` sees exactly
what a player sees — a `View` and a `Frame` — is what makes a bot filling an
empty seat in a real match and a test asserting on a scenario the same code path.
Seating one needs a runtime that can drive a seat from
something other than `Present::intend`, and `corvid_app` has one seat, one input
snapshot, and every other seat submitting `Action::default()` forever. A trait
here would be a trait nothing could drive.

**WAV comparison.** [`images_agree`] is the perceptual half, and it has a
renderer to be pointed at. The one mixer in the workspace is `corvid_audio`'s,
which is `f32` arithmetic on a device's own thread — the production path rather
than a deterministic one — so there is no bit-identical waveform here to compare
against. A byte comparison would be the wrong shape for it anyway, for the reason
`Tolerance` gives about pictures.

**`Scenario`, `agent::*`, `digest_at`, `screenshot` and `audio`.** A scenario
that seats two policies against each other and hands back a run to ask for the
digest at a tick, a screenshot and a stretch of audio is the shape this crate
does not have: its four functions are pointed at an `Opening` and an `Outcome`.

The five are one group rather than five gaps, because each rests on the same two
absences. `Scenario` and `agent::*` need a seat driven by something other than
`Controller::action`; `digest_at` is a `seek` over a
[`Session`](corvid_replay::Session) and could be written on its own, but the `run` that
would answer it is what `Scenario` returns; and `screenshot` and `audio` are the
PNG and WAV comparison above. A seat driven from outside the input snapshot, and
a reference mixer, are what stand between this crate and that shape.

[`Retention::Everything`]: corvid_app::Retention::Everything
[`BLESS`]: crate::BLESS
[`Diverged`]: crate::Diverged
[`How::Absent`]: crate::How::Absent
[`Mismatch::Unfrozen`]: crate::Mismatch::Unfrozen
[`What`]: crate::What
[`is_reproducible`]: crate::is_reproducible
[`images_agree`]: crate::images_agree
[`matches_goldens`]: crate::matches_goldens
[`Tolerance`]: crate::Tolerance
[`replays_to_itself`]: crate::replays_to_itself
