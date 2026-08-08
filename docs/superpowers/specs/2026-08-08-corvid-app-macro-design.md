# `corvid::app!` — a game's `main` is a declaration of its types

**Status:** approved design, unstarted
**Date:** 2026-08-08
**Branch:** `core-buildout`

## The whole of a game's `main`

```rust
corvid::app! {
    struct Pong;
    const PERIOD_MS: u8 = 33;
    type State = Table;
    type Controller = Hands;
    type Bot = Opponent;
    type Render = Graphics;
    type Auralizer = Ears;
}
```

That is the file. A window, a headless run, a replay, a save slot, a socket, a
seat and a table full of bots are all the same program, and this reads the
command line and decides.

## What is being built

Seven changes. The macro is the smallest of them and the last to write.

---

### 1. `Game`

The five types a game is, and how long its tick lasts, as one trait.

```rust
pub trait Game {
    /// How long one tick lasts, in whole milliseconds. Every peer must agree.
    const PERIOD_MS: u8;

    /// The span that follows, which is what the runtime steps against.
    const PERIOD: TickSpan = TickSpan::from_millis(Self::PERIOD_MS);

    type State: State + Opens;
    type Controller: Controller<Self::State>;
    type Bot: Controller<Self::State>;
    type Render: Render<Self::State>;
    type Auralizer: Auralizer<Self::State>;
}
```

`App<S, C, R, A>` becomes `App<G: Game>`; `Settings<S, C, R, A>` becomes
`Settings<G>`; `windowed::Pending<S, C, R, A>` becomes `Pending<G>`. One
parameter reaches every corner of `corvid_app` instead of four.

`Bot` is a second `Controller<Self::State>`. `impl Controller<S> for ()`
declares no actions, wants no devices and submits the idle action forever, so a
game with no bots names `()` and writes nothing.

The period lives here rather than on `State` because it is a property of the
whole game rather than of its simulation, and because the macro has somewhere to
put it.

**A period in whole milliseconds, not a rate in hertz.** `Step` accumulates
against the span, so the span is the number a simulation is defined by, and a
game that names hertz is naming a number that has to be divided into a second
and truncated. A `u8` of milliseconds spans 1 ms to 255 ms — a thousand ticks a
second down to four — and every value in it is exact.
`TickSpan::from_millis(u8)` is a new total constructor: no `NonZeroU32`, no
`match`, no fallback, because zero is not in the range and 255 ms fits a `u64` of
nanoseconds with room to spare.

pong's 30 Hz becomes 33 ms, which is 30.3 ticks a second. `TickSpan` is not in
the `Opening`, is not hashed and is not on the wire, so this cannot move a
digest — `baseline.rs` is unaffected by the change of period.

---

### 2. `Arguments`

```rust
/// What the run opens on.
pub enum Load {
    /// A level reference as a JSON-encoded string.
    Level(String),
    /// A save slot.
    Save(SaveSlot),
    /// A recorded session to carry on.
    Demo(PathBuf),
}

pub struct Arguments {
    /// Open no window, no adapter and no audio device.
    pub headless: bool,
    /// Claim no seat: submit nothing, watch what the others do.
    pub spectator: bool,
    /// How many unclaimed seats the game's `Bot` should play.
    pub num_bots: u16,
    /// Stop once this many ticks have run, counted from where the run opened.
    pub ticks: Option<Ticks>,
    /// What to open on, rather than the game's own opening.
    pub load: Option<Load>,
    /// Where to write the session, so that `--demo` can open it again.
    pub record: Option<PathBuf>,
    /// Where this game's files live, rather than the user data dir.
    pub state: Option<PathBuf>,
    /// Which seat this machine plays.
    pub seat: PlayerId,
    /// The UDP port to bind.
    pub listen: Option<u16>,
    /// Where the other machine is, as `HOST:PORT`.
    pub connect: Option<String>,
}
```

| flag | field |
|---|---|
| `--headless` | `headless` |
| `--spectator` | `spectator` |
| `--bots N` | `num_bots` |
| `--ticks N` | `ticks` |
| `--level JSON` | `load = Some(Load::Level(_))` |
| `--load N` | `load = Some(Load::Save(_))` |
| `--demo FILE` | `load = Some(Load::Demo(_))` |
| `--record FILE` | `record` |
| `--state DIR` | `state` |
| `--seat N` | `seat` |
| `--listen PORT` | `listen` |
| `--connect ADDR` | `connect` |

**`--record FILE` and `--demo FILE` are a pair.** One writes the session, the
other opens it. A run recorded on a build machine and a run played back on a
desk are the same file and the same two flags, and neither needs a harness.

`--record` writes the session only — the action log, the roster, the opening and
the digest trace. Writing it implies `Retention::Everything`, since a recording
of the last few seconds of an hour is not a recording. `App::capture(DIR)` is the
larger thing and stays a builder call: a directory with one audio frame and one
picture per displayed frame beside the session.

**`Load` is one field.** Two of these on one command line is
`Argument::Conflicting { flags: [_, _] }`, naming both, rather than a precedence
rule.

**`Load::Level(String)`** is JSON for `<S::Level as Level>::Reference`,
deserialized by the runtime and used in place of the opening's level. A string
because the command line has no types; the failure is
`Argument::NotALevel { value, why }`.

**`state`** is one directory holding everything this game writes on this machine:
`saves/`, `setting.json`, `binding.json`. `App::state` replaces `App::saves`, and
the three call sites that resolve a directory separately resolve one.

**`Ticks(u64)`** is a new newtype in `corvid_time`, beside `Tick` and `TickSpan`.
A count of ticks and a point in time are different things.

**`--bots N` with `--connect` is refused** as `Argument::Conflicting`. Two peers
each running their own bots would each write the same seats' columns, from
controllers that are not hashed and need not agree.

---

### 3. Playing a seat and watching one are different things

A client always watches a seat. Whether it also submits for one is what
`--spectator` decides.

```rust
pub enum Seating {
    /// Submits for this seat, and watches it.
    Playing(PlayerId),
    /// Submits for nobody, and watches this seat.
    Watching(PlayerId),
}

impl Seating {
    /// The seat this client's camera, renderer and ears belong to. Always one.
    pub const fn watched(self) -> PlayerId;
    /// The seat this client writes an action for, if it writes one.
    pub const fn playing(self) -> Option<PlayerId>;
}
```

`App::seat(PlayerId)` gives `Playing`; `App::spectating()` gives `Watching` of
the roster's first seat. `--spectator` is a bool, and which seat it watches is
not yet a flag.

What changes in the runtime is one thing: the log write. `Controller::update`,
`look`, `cursor` and `simulating` all still run, against `watched()`, so the
camera moves, the renderer draws and the ears hear exactly as they do for a
player. `Controller::action` is not called and nothing is written into the
frontier's row. With a transport, the peer joins as a listener: it folds in what
arrives and compares digests, and sends no actions of its own.

**A roster with no seats is `Error::NoSeats`.** There is nothing to watch, so
there is no run.

Because `watched()` is always a seat, `prepare`'s roster check stays as it is
and the camera path never sees an `Option`. Only the write is conditional.

---

### 4. Bots fill the empty seats

The runtime holds one `G::Bot`, built from its config, and calls it once per bot
seat with that seat's number. Bots take roster seats in order, skipping the seat
this client is `Playing`, up to `num_bots`. A `Watching` client skips nothing: a
spectator watches a seat it does not play, and a bot may play the seat it
watches.

`pong --seat 0 --bots 1` gives the bot seat 1. `pong --spectator --bots 2` gives
it seats 0 and 1, and the window shows seat 0's court while a bot moves its
paddle. Bot actions go into the log the same way this client's do, so they are
part of the session, written by `--record` and replayed by `--demo`.

---

### 5. Every hook takes one struct

The five hooks a game implements take a struct rather than a list of arguments,
so that a new thing to pass — a seat, a viewport, a frame index — is a field
rather than a signature change in every implementation in the workspace.

```rust
// corvid_behavior
pub struct Extracting<'a, S: State> {
    pub state: &'a S,
    pub level: &'a S::Level,
    pub time: Time,
}

// corvid_control
pub struct Acting<'a, S: State> {
    pub state: &'a S,
    pub input: &'a Input,
    pub time: Time,
    pub seat: PlayerId,
}

pub struct Updating<'a, S: State> {
    pub state: &'a S,
    pub input: &'a Input,
    pub loading: Option<Loading<'a, LevelRef<S>>>,
    pub time: Time,
    pub dt: Duration,
    pub seat: PlayerId,
}

// corvid_render
pub struct Opened<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub format: wgpu::TextureFormat,
}

pub struct Drawing<'a, S: State> {
    pub target: Target<'a>,
    pub camera: &'a Camera,
    pub loading: Option<Loading<'a, LevelRef<S>>>,
    pub time: Time,
    pub alpha: Factor16,
}

// corvid_sound
pub struct Hearing<'a> {
    pub out: &'a mut AudioFrame,
    pub camera: &'a Camera,
    pub time: Time,
}
```

```rust
fn extract(&mut self, extracting: Extracting<'_, S>);

fn action(&self, acting: Acting<'_, S>) -> S::Action;
fn update(&mut self, updating: Updating<'_, S>);
fn rumble(&self, acting: Acting<'_, S>) -> Option<RumbleId> { None }

fn new(opened: Opened<'_>, config: Self::Config) -> Self;
fn draw(&mut self, drawing: Drawing<'_, S>);

fn hear(&mut self, hearing: Hearing<'_>);
```

Fields are plain and public, not `#[non_exhaustive]`. A game reads them, so
adding a field costs a game nothing; the runtime and the handful of tests that
call a hook directly construct them, and those are the sites that should notice.

`seat` on `Acting` and `Updating` is how one `Bot` instance answers for several
seats. A seat is not something a tick may read, so it is here and not on `Time`.

---

### 6. `corvid::game!`

Defines the struct and its `Game` implementation, and nothing else.

```rust
corvid::game! {
    struct Rally;
    const PERIOD_MS: u8 = 33;
    type State = Table;
    type Bot = Opponent;
}
```

Every `type` line is optional and defaults to `()`; lines may appear in any
order; `struct` names the type. `PERIOD_MS` is required — a game that did not
say how long a tick lasts would inherit one, and two games sharing a default is
how two peers end up at different spans without either having said so.

It also generates a sandbox constructor:

```rust
impl Rally {
    /// A headless run with a scratch state directory and no settings file read.
    fn app() -> App<Self> { … }
}
```

which is the thing a test wants: deterministic, filesystem-free, and one call
rather than the seven builder lines every test file repeats.

### 7. `corvid::app!`

`game!` plus a `main`.

```rust
corvid::app! {
    struct Pong;
    const PERIOD_MS: u8 = 33;
    type State = Table;
    type Controller = Hands;
    type Bot = Opponent;
    type Render = Graphics;
    type Auralizer = Ears;
}
```

expands to the `game!` output plus

```rust
fn main() -> corvid::Result {
    corvid_app::main::<Pong>()
}
```

`corvid_app::main::<G>` does, in order:

1. `watch()`.
2. Parse. `--help` writes the usage to stdout and answers `Ok(())`.
3. Build the `App`: `G::State::opening()`, `G::PERIOD`,
   `Input::new(G::Controller::SETS)`, the `Seating`, the number of bots, and
   `G::Controller::bindings()` for a windowed run.
4. Open the transport, when `--listen` and `--connect` are both given.
5. Pick the backend from what the run is doing rather than from a `cfg`: a window
   when one was not refused and the build has the feature; an adapter drawing
   offscreen when there are pictures to write and no window; nothing otherwise.
6. Run.
7. Report.

**Reporting.** Everything the framework has to say goes through `tracing`: one
`corvid_app.finished` event carrying the ending tick, the settled tick, the
digest, the `Traffic` counters and the request count. A headless run also writes
the settled digest, and nothing else, to stdout — that is the program's answer to
a pipe rather than an event.

The **settled** digest: on a networked run the newest ticks were simulated partly
from predictions, so two peers that stopped a second apart hold different states
for the same session. The settled tick is far enough back that every seat's real
action was in hand, and it is derived from `Budget`, so it lives beside the budget
rather than as a constant in a game.

---

## What pong becomes

`main.rs` is the macro invocation and its module documentation.

`--demo` and `--together` go away. `rally::Match`, `rally::together` and
`rally::Policy` stay as library code driven by `tests/linked.rs` and
`tests/together.rs`, so the netcode is exhibited by assertions rather than by a
flag somebody has to run.

`Hands` loses `scripted` and its `Config` becomes `()`. `Opponent` is a type in
`bot.rs` implementing `Controller<Table>` with `REAL = false` and `SETS = &[]`,
wrapping `target` and `toward` and reading `acting.seat`. `rally::Racket`
collapses onto it. `pong::RATE` and the `rate()` helper go: the period is in the
macro, and 30 Hz becomes 33 ms.

`pong --headless --bot` becomes

```
pong --headless --spectator --bots 2
```

which is a whole session rather than one scripted paddle: both seats played,
nobody submitting, one digest on stdout.

pong's bespoke scoreline goes with `report`. A game with something to say about
its own state emits a `tracing` event from its client-local code, which sees
every state through `Extract`.

## Testing

- `corvid_app/tests/arguments.rs` rewritten: every flag, the two-`Load` conflict,
  `--bots` with `--connect`, a malformed `--level`.
- `--record FILE` writes a session that `--demo FILE` opens and carries on, and
  the digest trace joins up across the two runs.
- `--spectator` writes no action for this client and reaches the same digest as a
  run whose seat submitted `Action::default()`.
- A `--spectator` run still calls `update`, `look` and `draw` against the watched
  seat: the camera is not the default one, and an offscreen `--spectator` run
  writes the same pictures as a played one at the same state.
- `--spectator` on a roster with no seats is `Error::NoSeats`.
- `--bots N` fills the expected seats, and the bot's actions are in the recorded
  session.
- `TickSpan::from_millis` is exact across the whole `u8` range, and a session at
  33 ms produces the digests a session at `from_hz(30)` did.
- A run with `--bots 1` and a run whose second seat is driven by the same
  `Opponent` through a hand-built `App` produce identical traces.
- The `corvid_app` test files that build their own `App` move to `game!` plus
  `App::<G>::new()`, which is the change that says whether the test macro earns
  its place.
- pong's `session.rs`, `socket.rs`, `bot.rs` and `baseline.rs` move off
  `Hands::scripted` and onto `Opponent`. `baseline.rs` keeps its own copy of the
  scripted paddle and its digests must not move.

## Risks

**The listening peer.** `corvid_lockstep` assumes a peer submits. `Seating`
keeps the camera path free of `Option`s and confines the change to the log
write, but a `Peer` that sends nothing may still need something inside
`corvid_lockstep` — that is the first thing to find out, and the one place this
spec could be wrong about scope.

**`baseline.rs`.** Its digests are the evidence that none of this changed the
simulation, and they must survive untouched. It holds its own copy of the
scripted paddle and drives a hand-built `App`, so it does not depend on `Hands`
or on the flags; what it does depend on is the seat parameter arriving in
`action` and the argument structs replacing the loose ones. Convert it first and
run it before anything else moves.

**Signature churn, once.** Every `Controller`, `Render`, `Auralizer` and
`Extract` implementation in the workspace and in `examples/` changes shape. It is
mechanical and wide, and it is the last such change: that is what the argument
structs are for.
