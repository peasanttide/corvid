# `corvid_app`

The loop. [`State`] says what a tick is, [`Render`] says what a client draws
one with, [`Controller`] says what else a client does with one and
[`Auralizer`] says what it hears; this crate is the thing that calls all four,
in a fixed order, at a fixed rate, and writes down what happened.

It is the first crate in [Corvid](https://github.com/peasanttide/corvid) that
opens a file or reads a clock, and that is not an accident of layering: every
crate below it is `no_std` precisely because everything touching an operating
system was pushed up here. There is no `no_std` build of this one and no feature
that would make one.

```rust
use std::sync::Arc;

use corvid_app::App;
use corvid_control::Controller;
use corvid_behavior::{ProfileId, State};
use corvid_replay::{Opening, Profile, Schema, Seed, Snapshots};
use corvid_time::Tick;
# use corvid_behavior::{Command, Level, Malformed, Player, Source};
# use serde::{Deserialize, Serialize};
#
# #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
# struct Nowhere;
# impl Level for Nowhere {
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
#     type Level = Nowhere;
#     type Rules = ();
#     type Action = Effort;
#
#     fn tick(
#         self,
#         _level: &Nowhere,
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
# /// The climber, who always climbs.
# ///
# /// A controller is not optional for a game whose actions matter: `App`
# /// defaults `C` to `()`, and `()` submits `Action::default()` forever — which
# /// for this game is `Effort::Rest`, so the run below would never finish.
# #[derive(Clone, Copy, Debug, Default)]
# struct Legs;
# impl Controller<Climb> for Legs {
#     type Config = ();
#     const SETS: &'static [corvid_input::SetDescriptor] = &[];
#     fn new((): ()) -> Self { Self }
#     fn configure(&mut self, (): ()) {}
#     fn action(&self, _: &Climb, _: &corvid_input::Input, _: corvid_behavior::Time) -> Effort {
#         Effort::Up
#     }
#     fn update(
#         &mut self,
#         _: &Climb,
#         _: &corvid_input::Input,
#         _: Option<corvid_behavior::Loading<'_, String>>,
#         _: corvid_behavior::Time,
#         _: core::time::Duration,
#     ) {}
#     fn look(&self) -> corvid_camera::Camera { corvid_camera::Camera::default() }
# }
#
// A climber who gains a metre per tick. `State` and `Controller` for `Climb`
// are elided here; the two contracts carry their own worked examples.
fn opening() -> Opening<Climb> {
    let schema = Schema::new("climb").field("State.metres", "i64").digest();
    Opening::<Climb> {
        level: "cliff".to_owned(),
        content: Arc::new(Nowhere),
        rules: Arc::new(()),
        roster: vec![Profile { account: ProfileId(1), joined: Tick::ZERO, left: None }],
        seed: Seed(1),
        first: Tick::ZERO,
        origin: None,
        schema,
    }
}

let run = App::<Climb, Legs>::new()
    .headless()
    .opening(opening())
    .until(|state: &Climb, _at: Tick| state.metres >= 100)
    .run()?;

// A hundred ticks, no window, and no clock to wait for.
assert_eq!(run.state.metres, 100);
assert_eq!(run.session.last(), Tick(100));

// The predicate is handed the tick as well as the state, so "a hundred ticks"
// needs no counter in the game's own state — and when that is the whole of the
// condition, `for_ticks` says it in one word.
let counted = App::<Climb, Legs>::new()
    .headless()
    .opening(opening())
    .for_ticks(100)
    .run()?;
assert_eq!(counted.session.last(), Tick(100));
assert_eq!(counted.state, run.state);

// And the session it leaves replays to the state it stopped at, which is what
// makes the run worth recording.
let (replayed, _) = run
    .session
    .seek(&mut Snapshots::new(0), run.session.last())?;
assert_eq!(replayed, run.state);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## The loop

One iteration is one reading of the clock. The [`Step`] turns the elapsed time
into a whole number of owed ticks, each of those runs, and then exactly one
frame is displayed — including on the iteration the run stops on, so the last
tick a capture holds a state for is also the last tick it holds a frame for.

Before any of it, the loop asks the game two questions the view answers.
[`Controller::simulating`] decides whether the step is advanced at all: `false` is
a pause, and the elapsed time is *discarded* rather than accumulated, so ten
minutes on a menu are followed by one ordinary tick rather than by the catch-up
ceiling's worth all at once. Everything in the second table below still happens,
because a pause screen has to be drawn and navigated. And the backend is asked
how big its target is, which goes into the input snapshot as
[`Input::viewport`] — the rectangle a pointer is reported against, and the one
thing a snapshot cannot know about itself. A run with no target answers
[`None`], because a headless run genuinely has no viewport.

| Per owed tick | |
|---|---|
| 1 | `intend` builds this client's action from the view, the frame and the input |
| 2 | the action goes into the [`ActionLog`], at this tick, against this client's seat |
| 3 | `tick` runs against the roster the session says was seated, read back out of that log |
| 4 | the new state is digested into the [`HashTrace`] |
| 5 | the commands the tick returned are drained into the sink |

| Per displayed frame | |
|---|---|
| 1 | `look` advances the view by the interval the clock reported |
| 2 | `hear` refills the one [`AudioFrame`] the run keeps |
| 3 | the backend takes the view, the frame and the audio |
| 4 | a backend with a device acquires a target, opens an encoder, and calls the game's [`Render::draw`] into it |

Step four is why the backend takes a view and a frame rather than a picture.
There is no picture: a game records `wgpu` calls, and the encoder they go into
does not exist until something has acquired somewhere to draw. So the call
order is inverted from what a draw list allowed — the backend calls the game
rather than the other way round — and a backend with no device never calls it
at all.

Step three is worth reading twice. The loop writes this client's action into the
log and then reads the *whole row* back, so the actions a tick sees are the
actions a replay of the same session sees, by construction rather than by
agreement. What that costs is that a loop which forgot step two would still be
self-consistent — it would play a game in which this client did nothing, and
replay to itself perfectly. `tests/capture.rs` names that and checks the log's
confirmation bits rather than trusting the digests to see it.

Nothing in either table hands a retired state back to the game. The loop holds
both ends of the pair the display sits between, so what falls out of the far end
when a tick lands is one state older than what [`Session::seek`] lets go of at
the same point — and a state is an `Arc` now, so neither of them can promise it
is the last handle. An extractor is handed the state by reference for the
length of the call — [`Extract::extract`] takes `&S` — so it copies out what its
own device wants rather than keeping anything, and what it copies out can be
written down, hashed and replayed with no runtime behind it.

## A headless run does not depend on wall time

The only clock is the one the [`App`] was given, and the default is
[`Fake::stepping`] at the tick rate's period: one period per reading, forever. A
run driven by it owes exactly one tick per iteration, finishes as fast as the
processor manages, and produces the same session every time.

`tests/headless.rs` is where that is made falsifiable rather than asserted. Two
runs agreeing with each other only says the loop is a function of *something*,
so a frozen table of digests says which function; and because a wall clock
reaching `look` would move the actions this client submits — the fixture game
folds the simulated seconds it has been handed into its own `intend`, which
`corvid_present` warns is a display-rate quantity reaching an action — a run that
read a real clock records a different action at two named ticks. The timing claim
is checked as a bound rather than as a number: three hundred ticks of a
twenty-second game finish in under a tenth of the time they simulate, with two
orders of magnitude of margin, so the test is about whether the loop waits for
anything at all rather than about how fast a machine is.

## What a capture holds, and what it does not

`capture(directory)` writes four kinds of file. Three go through `corvid_wire`,
which is the workspace's one encoding and the one [`Session::load`] reads; the
fourth is a PNG, because there is no other way to write down what a device
drew.

| Path | What |
|---|---|
| `frames/<tick>.png` | the frame a device drew, read back — **offscreen runs only** |
| `audio/<tick>` | the [`AudioFrame`] `hear` produced for that tick |
| `trace` | the [`HashTrace`]: one digest per tick |
| `session` | the whole [`Session`], which is what a replay needs |

The trace is in the session as well, and it is written twice on purpose:
diffing one build's marks against another's is the common operation and it
should not mean decoding a whole session — which carries the level, the rules
and the opening state — to reach a column of digests.

A frame is named for the tick its `current` state is at, so a run displaying
several frames between two ticks leaves the last one under that tick's name. The
directory is created if it is not there and written into if it is; nothing here
removes a directory somebody named, so capturing over a longer run leaves that
run's later frames behind.

Whether there is a frame at the *opening* tick is a property of the clock and
not of this loop. "The first frame is displayed after the first tick" is true of
the default clock and of nothing else. One iteration is one reading, and a reading owes
`elapsed / period` ticks; a clock slower than the tick rate owes zero on its
first readings and the loop displays anyway, because a display that waited for a
tick would be a display that stutters whenever the simulation is not due. So a
run driven by a quarter-period clock writes `audio/0` before any tick has run,
and a run driven by the default clock does not. `tests/capture.rs` runs both and
asserts the difference rather than the claim.

**Only an offscreen run writes a picture, and it is a weak golden.** Reading a
frame back needs a texture that is still there afterwards, which
[`App::offscreen`] has and a window does not — a presented frame belongs to the
compositor. So `frames/` is created by every capture, filled by an offscreen
one, and left empty by a headless or windowed one.

What is in it is what a *driver* rasterised, which is not the same kind of
evidence as the rest of the directory. Two drivers disagree about the last bit
of a shaded pixel for reasons that have nothing to do with the game, so a PNG is
compared with a tolerance — `corvid_test::Tolerance` — and its exact-match arm
belongs to a machine that knows which adapter it has. The bit-exact golden here
is `trace`, and it is compared byte for byte on every target.

**No waveforms.** Nothing here hands a frame to an audio device, so a capture
records the [`AudioFrame`] and nothing turns it into samples. A `Screenshot`
request is still answered by recording that it was made rather than by writing a
file.

**No saves either.** A `Save` request's bytes are kept for the length of the run,
so a `Read` in the same run finds them, and they are not written to the capture
directory. Putting a game's save file somewhere is a platform's job and there is
no platform layer under this crate.

## What a run keeps, and what it lets go of

A run writes a row of actions and a digest every tick. A run that never seeks
reads neither again, and an hour at [`TickRate::CRADLE`] is 54 000 of each —
fifteen a second for sixty minutes. Keeping all of that against the possibility
that somebody drags a slider is not a default a game should have to opt out of,
so the default is bounded and keeping everything is something a run asks for.

| | What the session holds | What it costs |
|---|---|---|
| *(default)* | [`Retention::RECENT`]: at least 256 ticks, never more than twice that | one more handle to a state the loop is already holding, per window |
| `retain(Retention::Recent { ticks })` | at least `ticks`, never more than twice | the same, per window |
| either of those, run for fewer than `ticks` ticks | every tick it played, which is all there was | nothing: nothing has been forgotten yet |
| `retain(Retention::Everything)` | every tick from the opening | memory that grows with the run |
| `capture(directory)` | every tick, unless `retain` says otherwise | the same |

A capture keeps everything because a capture is a request to write the run
down, and a recording of the last seventeen seconds of an hour is not the
recording anybody asked for. `retain` beats that in either direction and
whichever order the two calls are written in, so a soak test that wants a
bounded capture says so and gets one.

**What a bounded run gives up is reach, and nothing else.** Save, replay,
rollback and time-walk are one [`Session::seek`] over whatever the session
holds, so a default run can still do all four — over its window rather than over
the whole run. What is gone is gone rather than wrong: a seek to a tick before
the window is [`Unreachable::Before`], not a state built from actions nobody has.

| | A default run can |
|---|---|
| Save | yes: [`Session::save`] writes what it holds, and it loads and replays |
| Replay | yes, from the tick the session now opens at to the tick it is on |
| Rollback | yes: a correction anywhere in the window re-simulates from it |
| Time-walk | as far back as the window, and no further |

The window is a range rather than a number because of what the tight version
costs. Forgetting the row at exactly `now - ticks` on every tick means holding
the state at `now - ticks` on every tick — a ring of whole states, which is a
hundred megabytes for a game with fifty thousand entities and is the thing
[`Snapshots`] is deliberately sized in bytes for. Instead the loop sets one state
aside every window and forgets back to the previous one, so what a bounded run
pays is one more handle to a state it is already holding and what it holds
sawtooths between one window and two. The state that falls out of reach is
dropped, and the memory it owned comes back with the last handle to it.

`tests/retention.rs` is where the numbers above come from: it runs the same
opening bounded and unbounded and compares the state, the marks and the actions
over the overlap, measures what a run of 213 ticks with a window of 23 actually
held, and seeks, saves, replays and rolls back the session a bounded run left.

## When a run stops

Three things stop a run, and a run that names none of them does not return.

| | |
|---|---|
| `Quit` | a tick asked to, and the [`Outcome`]'s status is the one it named |
| `until` | the predicate said so, given the state a tick produced **and that state's tick** |
| `for_ticks(n)` | `n` ticks have run, counted from the opening's first tick |

The tick in `until` is there because of what its absence cost. A predicate that
was handed only the state could not say "a hundred ticks" without the game
keeping a counter — and a game's counter lives in `State`, which is hashed,
serialized and sent, so a column that exists for a test's benefit is a column
every peer exchanges every tick. `tests/common/mod.rs`'s third fixture has no
such counter, and the tests that use it are runs of a fixed length.

`for_ticks` is the same condition written once. It is a count rather than a
predicate, which is what lets `for_ticks(0)` be a run of no ticks: the count is
checked on both sides of a tick, so zero stops before the first one and `n`
stops on the iteration whose tick reached `n` rather than one iteration later.
Naming both a count and a predicate is allowed and stops at whichever comes
first; a `Quit` beats both, because a tick that asked to quit has already run.

## The front door

A game's `main` is [`main`], and there is no second spelling:

```rust,ignore
fn main() -> corvid_app::Result {
    corvid_app::main::<Bounce>()
}
```

A window, a headless run, a capture, a replay and a save slot are all the same
program: `main` reads the process's arguments and decides. **A game never asks
for determinism**, because a game that had to call `.headless()` would have a
mode that is deterministic and a mode that is not, and only one of them would be
tested.

| | |
|---|---|
| `--headless` | play with no window, no adapter and no audio device |
| `--ticks N` | stop once `N` ticks have run, counted from where the run opened |
| `--capture DIR` | write the run down under `DIR` |
| `--retain N` \| `--retain all` | keep at least `N` ticks of the session, or all of it |
| `--replay FILE` | open on the session recorded in `FILE` |
| `--load N` | open on save slot `N` |
| `--saves DIR` | put the save slots under `DIR` rather than under `$XDG_DATA_HOME/NAME/saves/` |
| `--help`, `-h` | the usage |

Every one of those is a thing the *operator* decides: whether this machine has a
display, how long to run for, whether to record it, how much to keep, which
recorded run to open on. A setting only the game can know — its opening, its
rules, its passes — is not here and should not be, because a flag for it would
be a flag whose legal values only the game could list. `--ticks 100` and
`--ticks=100` are the same argument, and a flag that takes a value and is given
none is refused rather than defaulted, because "zero ticks" and "as long as you
like" are both things somebody might have meant.

`main` is one function with one bound, `G: Present` — which is `G: Render`,
because the client-local half is a chain of traits over one marker. A game that
draws nothing writes `type Graphics = ();` and satisfies it. There used to be two
definitions under opposite `cfg`s and a trait reconciling their bounds, and both
existed because a `wgpu` type could not be named a crate lower down; it can now.
[`App`] is still here for a harness driving a run by hand, and [`App::launch`]
is the same reading of the command line for one.

## Save and load, without a game asking

`Command::Save` and `Command::Read` are in the closed vocabulary already, and
the runtime is what acts on them. A slot is resolved against `--saves DIR` if
the operator named one and `$XDG_DATA_HOME/NAME/saves/` otherwise, from
[`State::NAME`](corvid_behavior::State::NAME); what goes in it is the
session and the state, through `corvid_wire`; and reading one back is
[`Session::seek`](corvid_replay::Session::seek) — the same call rollback and
time-walk are, so a save that cannot be replayed is refused at the load rather
than a hundred ticks later. **A game implements nothing for this.** Its `State`
is `Data`, and that is the whole requirement.

The default follows the XDG Base Directory specification: an absolute
`XDG_DATA_HOME` if the environment sets one, `$HOME/.local/share` if it does
not, and `%APPDATA%` on Windows. Written out rather than taken from a crate,
because the rule is those three lines. An environment that names no home at all
falls back to `./saves/NAME/`, which is where every run used to write.

The operator's word beats the builder's, whichever order the two are written in,
and an argument nobody gave changes nothing — so a game keeps every default it
set. That is why [`App::arguments`] is the one setter on the builder that does
not take effect where it is written: it records the command line and [`App::run`]
applies it after every other call has had its say, because an ordinary setter
would be overwritten by a `for_ticks` two lines further down and the flag would
be silently ignored. A game that wants none of this calls [`App::run`] without
ever calling `arguments`, and the command line is never read.

**`--help` is not a failure, and [`main`] answers it.** An operator who asked for
the usage got what they asked for: [`main`] writes [`Arguments::USAGE`] to
**stdout** and answers `Ok(())`, so the process exits zero and a shell script
does not have to special-case it. It travels that far as an error only because
the parser that noticed it may not print — this crate denies the printing macros,
a library that reaches for somebody's stdout being one they cannot silence — so
[`Arguments::parse`] reports [`Argument::Help`], whose `Display` *is* the usage,
and [`main`] is the one place in the crate that writes it. A harness driving a
run through [`App::launch`] rather than through [`main`] gets the `Help` back and
does as it likes with it. Nothing here takes a command-line parsing dependency:
seven flags, no subcommands, and no completion is less code than the manifest
entry would be.

## The command sink

Four requests are acted on, and every other one is accepted, recorded as
unhandled, and warned about at `WARN`. Never a panic, and never a silent drop: a
tick that asks for a rumble is a correct tick, and a runtime with no rumble in it
is a runtime with a gap.

| Request | What happens |
|---|---|
| `Quit` | the loop stops at the tick that asked, and its status is the [`Outcome`]'s |
| `Save` | the bytes are kept, in memory, under the slot |
| `Read` | the slot is looked up; `Answer::Empty` says there was nothing there |
| `Screenshot` | recorded, with the tick that asked |
| everything else | recorded as `Answer::Unhandled`, with a warning |

"At the tick that asked" is the boundary a test pins from both sides. The tick
that returns a `Quit` is a tick that *ran*, so the state after it exists and no
tick after that one does — and moving the request one tick later moves the last
state one tick later and nothing else.

Routing is [`Scope`]'s and not a second classification: a global request
is one the session makes and every peer has to agree about, and a local one
belongs to one machine. This runtime runs one peer in one process, so both
kinds are acted on here, which is exactly why the scope is *recorded* against
each request rather than merely consulted. A `Quit` this peer agreed to alone
reads no differently from one every peer agreed to until there is a second peer,
and the record is what a lockstep runtime would have to reconcile.

## The `dev` feature

`corvid_behavior` requires that whatever a tick reads out of its `Scratch` be a
pure function of the values its arguments denote — a memo, never an accumulator —
and no type can say so. The cost of getting it wrong is delayed and it lands
somewhere else: a game that accumulates in its scratch plays and replays
identically from the opening, and comes apart during a rollback, in front of a
player, on one machine, with nothing on the wire to blame. `tests/dev.rs`
reproduces that in every build, feature or no feature, by seeking one session
twice under two snapshot budgets and getting two different states.

Under `dev` the runtime stops trusting the obligation. On a schedule that is a
function of the session's [`Seed`] and the tick number and of nothing else, it
throws the accumulated scratch away and calls `tick` with `Scratch::default()`
instead. A game that reads scratch history then diverges during ordinary play, at
a tick anyone can name, rather than during a rollback.

**A `dev` peer and a release peer do not agree.** That is the price and it is not
a bug: the two configurations compute different states for a game that leaks,
which is what the check is for. What the schedule buys is that two `dev` peers
*do* agree with each other, because it depends on nothing either machine knows —
so a team can play a `dev` build together. It is off by default for that reason,
and a mixed session is a desync.

The schedule is [`dev::discards`], and it is public so that a test or a tool can
say when it will next fire. It folds the seed and the tick through the digest
rather than being `tick % 4`, so a game whose own behaviour has a period cannot
line up with it.

## What this crate enforces, and what the caller owes

Worth separating, because most of what makes a run reproducible is not something
this crate can check.

What it **enforces**: the loop's only source of real time is the [`Clock`] the
app was given, so a run driven by a fake clock is a run whose ticks and whose
`look` intervals are the fake's; the roster is rebuilt from the opening and the
log every tick rather than remembered, so there is no fourth input a capture does
not record; the action a tick sees comes out of the log; and a run refuses to
start when the seat it submits for is not in the roster, because a run recording
its actions nowhere would replay as a run in which this client did nothing.

What the **caller owes**: everything `corvid_behavior` and `corvid_present` say
they owe, and this is the call site that makes all of it load-bearing. A
`Scratch` that accumulates, a `Clone` that is not a copy, a `look` that writes a
`State` through interior mutability, an `intend` whose action names a screen
pixel — none of
those is refused here, and each of them is a run that does not replay to what it
ran. A `static` with interior mutability survives every check in this workspace;
what would find one is two peers that are genuinely two processes comparing
digests, and nothing here runs two processes.

## Three backends, one loop

| Builder | Where a frame goes | What pulls in |
|---|---|---|
| *(default)* | nowhere, or into a capture | nothing |
| `offscreen(size)` | a texture, through `corvid_render` | `wgpu` |
| `window()` | a window's surface | `wgpu` and `winit` |

A window takes its title from [`State::NAME`](corvid_behavior::State::NAME)
and its icon from [`Render::icon`], so neither is a builder argument: a game
that spelled its name once for a title bar and once for the directory its saves
land in would have two names.

The loop is the same in all three. `Backend` — the trait the three implement —
has one interesting method, it takes a tick, a view, a frame and an audio frame,
and it answers `Result<(), Error>`: **there is nowhere in that signature to
return a state, a tick, an action or a digest.** Nor is there one on
`Render::draw`, which is what a backend with a device calls: it returns nothing
at all. So whichever backend a run uses, the trace it records is the same trace,
and that is a property of the types rather than a convention.
`tests/windowless.rs` runs the same opening with an adapter actually rasterising
every frame and without one, and compares the marks, the log, the state and the
requests.

The two settings that need a device are ordinary builder calls with no bound of
their own. They used to sit in an impl block asking for `G: Render` while the
rest of the builder asked for `G: Present`, because those were two bounds; they
are one bound now, and a determinism check that never opens a device is a run of
a game that has a `draw` and does not call it.

A windowed run differs in two more ways, both documented on the builders that
cause them. Its input snapshot is refilled from the window's devices every
frame, so the snapshot given to `input` supplies the *declaration* and not the
values. And its default clock is [`Wall`] rather than [`Fake::stepping`],
because a window in front of a player runs in real time.

The event loop owns `main` on a windowed run, because on iOS, Android and the
web it has to — see `corvid_window`. `run` still returns an `Outcome`; what
changes is that it does not return until the window closes.

`headless()` undoes both of the above. A game does not call it — `--headless` is
the operator's, and [`main`] makes the call for them — and what it is here for
is a harness that wants a run with no device whatever the machine has.

## What this crate does not do

There is no device layer on a headless run, so the [`Input`] handed to `intend`
and `look` is the one the app was given and nothing refills it.
There is no audio backend, so a windowed run is silent: `hear` still fills an
[`AudioFrame`] and a capture still records it, and nothing turns it into
samples. There is one peer, so this client's seat is the only column of the log
anything writes and every other seat holds `Action::default()` forever. A run
with no `until` whose game never asks to quit does not return — nothing here can
decide that for a caller, and on a windowed run that is the ordinary case, since
closing the window is what stops it. And the binding a windowed run plays with
is `Bindings::placeholder`, which binds by identifier number and has no idea
what any action means; there is no per-device, rebindable table with glyphs in
it, and `corvid_window` says so at length.

## Features

| Feature | Effect |
|---|---|
| `dev` | The scratch-discarding schedule above. It changes what a build **computes** rather than what it exposes |
| `render` | A device: [`App::offscreen`], the [`Render`] bound on [`main`], and the `corvid_render` and `corvid_mesh_render` dependencies |
| `window` | A window, which implies `render`: [`App::window`], [`App::bindings`], and the `corvid_window` dependency |

`render` and `window` are off by default because `wgpu` and `winit` together are
most of what a graphics stack weighs, and a dedicated server, a determinism check
and a game's own `cargo test` want none of it. A game that draws turns one of
them on; a game that does not writes `type Graphics = ();` in its `Present`
implementation, writes `type Graphics = ();` for the rest, and never compiles
either.

There is no derive feature and nothing else to turn on: a game's state is marked
with `#[derive(Hash)]`, which is `core`'s.

**A game does not name this crate: it names `corvid`.** That is the workspace's
one facade, and it gathers everything a run needs — the two contracts, the
session, the maths, the audio frame, the input snapshot and the renderer — from
the crates that own them. This one is an ordinary crate below it, holding the
loop and the entry points, and it re-exports nothing.

That changed. Every crate here used to forward its neighbours, so a `Factor32`
could be reached as `corvid_app::present::transform::Factor32`, as
`corvid_render::transform::Factor32`, and by two other paths besides — four
spellings of one type, with nothing to say which was meant. There is one now,
and it is `corvid::Factor32`.

[`flavour`] is the other half of what a feature costs. A capture recorded by a
`dev` build and one recorded by a release build are recordings of two different
computations whenever the game reads its scratch, so anything freezing a capture
as a golden has to record which of the two it blessed. This crate cannot do that
for a caller — a capture directory is a caller's format — so it publishes the
name and says here that a golden which does not carry it is a golden that will
be compared against the wrong build eventually.

[`App::arguments`]: crate::App::arguments
[`App`]: crate::App
[`App::launch`]: crate::App::launch
[`main`]: crate::main
[`Render::icon`]: corvid_render::Render::icon
[`Controller::simulating`]: corvid_control::Controller::simulating
[`Input::viewport`]: corvid_input::Input::viewport
[`App::run`]: crate::App::run
[`Argument::Help`]: crate::Argument::Help
[`Arguments`]: crate::Arguments
[`Arguments::USAGE`]: crate::Arguments::USAGE
[`Retention::RECENT`]: crate::Retention::RECENT
[`Session::save`]: corvid_replay::Session::save
[`Snapshots`]: corvid_replay::Snapshots
[`TickRate::CRADLE`]: corvid_time::TickRate::CRADLE
[`Unreachable::Before`]: corvid_replay::Unreachable::Before
[`flavour`]: crate::flavour
[`ActionLog`]: corvid_replay::ActionLog
[`AudioFrame`]: corvid_sound::AudioFrame
[`Clock`]: corvid_time::Clock
[`Scope`]: corvid_behavior::Scope
[`Render::draw`]: corvid_render::Render::draw
[`App::offscreen`]: crate::App::offscreen
[`Fake::stepping`]: corvid_time::Fake::stepping
[`HashTrace`]: corvid_replay::HashTrace
[`Input`]: corvid_input::Input
[`Controller`]: corvid_control::Controller
[`Seed`]: corvid_replay::Seed
[`Session`]: corvid_replay::Session
[`Session::load`]: corvid_replay::Session::load
[`Session::seek`]: corvid_replay::Session::seek
[`Extract::extract`]: corvid_behavior::Extract::extract
[`State`]: corvid_behavior::State
[`Auralizer`]: corvid_sound::Auralizer
[`Step`]: corvid_time::Step
[`Wall`]: corvid_time::Wall
[`Arguments::parse`]: crate::Arguments::parse
[`App::window`]: crate::App::window
[`App::bindings`]: crate::App::bindings
[`Render`]: corvid_render::Render
