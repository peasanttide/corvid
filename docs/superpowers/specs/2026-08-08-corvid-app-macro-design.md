# `corvid::app!` — a game's `main` is a declaration of its types

**Status:** approved design, unstarted
**Date:** 2026-08-08
**Branch:** `core-buildout`

## The whole of a game's `main`

```rust
corvid::app! {
    struct Pong;
    const RATE: u32 = 30;
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

The five types a game is, and the rate it ticks at, as one trait.

```rust
pub trait Game {
    /// How often a tick runs. Every peer must agree.
    const RATE: TickSpan;

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

The rate lives here rather than on `State` because it is a property of the whole
game rather than of its simulation, and because the macro has somewhere to put
it.

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

### 3. A seat is optional

`--spectator` claims no seat. `App::seat(PlayerId)` gains a sibling
`App::spectating()` which clears it, and the field behind both is
`Option<PlayerId>`.

- `prepare` skips the `Error::Seat` roster check.
- The loop writes no action into the log for this client. The column is filled by
  a peer or a bot, or stays `Action::default()`.
- With a transport, the peer joins as a listener: it folds in what arrives,
  compares digests, and sends no actions of its own.

This is the deepest change here and the one most likely to surface an assumption
in `runtime.rs` or `corvid_lockstep`.

---

### 4. Bots fill the empty seats

The runtime holds one `G::Bot`, built from its config, and calls it once per bot
seat with that seat's number. Bots take unclaimed roster seats in order, skipping
this client's, up to `num_bots`.

`pong --seat 0 --bots 1` gives the bot seat 1. `pong --spectator --bots 2` gives
it both. Bot actions go into the log the same way this client's do, so they are
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
    const RATE: u32 = 30;
    type State = Table;
    type Bot = Opponent;
}
```

`RATE` is hertz. Every `type` line is optional and defaults to `()`; lines may
appear in any order; `struct` names the type. The macro turns the hertz into a
`TickSpan` through a `const` match on `NonZeroU32`, so a game never writes that
itself and no `unwrap` appears anywhere near a rate.

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
    const RATE: u32 = 30;
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
3. Build the `App`: `G::State::opening()`, `G::RATE`,
   `Input::new(G::Controller::SETS)`, the seat or the absence of one, the number
   of bots, and `G::Controller::bindings()` for a windowed run.
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
collapses onto it. `pong::RATE` and the `rate()` helper go: the rate is in the
macro.

pong's bespoke scoreline goes with `report`. A game with something to say about
its own state emits a `tracing` event from its client-local code, which sees
every state through `Extract`.

## Testing

- `corvid_app/tests/arguments.rs` rewritten: every flag, the two-`Load` conflict,
  `--bots` with `--connect`, a malformed `--level`.
- `--record FILE` writes a session that `--demo FILE` opens and carries on, and
  the digest trace joins up across the two runs.
- `--spectator` runs a session, writes no action for this client, and reaches the
  same digest as a run whose seat submitted `Action::default()`.
- `--bots N` fills the expected seats, and the bot's actions are in the recorded
  session.
- A run with `--bots 1` and a run whose second seat is driven by the same
  `Opponent` through a hand-built `App` produce identical traces.
- The `corvid_app` test files that build their own `App` move to `game!` plus
  `App::<G>::new()`, which is the change that says whether the test macro earns
  its place.
- pong's `session.rs`, `socket.rs`, `bot.rs` and `baseline.rs` move off
  `Hands::scripted` and onto `Opponent`. `baseline.rs` keeps its own copy of the
  scripted paddle and its digests must not move.

## Risks

**The absent seat.** `runtime.rs` and `corvid_lockstep` assume a seat to write
into. If the listener path needs changes inside `Peer`, that is larger than this
spec accounts for, and it is the first thing to find out.

**`baseline.rs`.** Its digests are the evidence that none of this changed the
simulation. They must survive every change here untouched, and the run that
produced them used `--bot`, which no longer exists — so the equivalent run under
the new flags has to be established before anything else moves.

**Signature churn, once.** Every `Controller`, `Render`, `Auralizer` and
`Extract` implementation in the workspace and in `examples/` changes shape. It is
mechanical and wide, and it is the last such change: that is what the argument
structs are for.
