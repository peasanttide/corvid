# `corvid::app!` — a game's `main` is a declaration of its types

**Status:** approved design, unstarted
**Date:** 2026-08-08
**Branch:** `core-buildout`

## The problem

`examples/pong/src/main.rs` is 385 lines. `corvid_app::main::<S>()` exists and is
one line, and pong cannot use it, because that function takes one type parameter
and pong is four types; because it decides the backend under `cfg` rather than
from what the run is actually doing; and because pong needs a seat, a socket and
a scripted paddle that no framework flag reaches.

So pong hand-writes an argument parser, a socket opener, a backend chooser and a
digest reporter — four things that are not about pong. Every game after it would
write them again.

The whole of a game's `main` should be:

```rust
corvid::app! {
    type State = Table;
    type Controller = Hands;
    type Agent = Bot;
    type Render = Graphics;
    type Auralizer = Ears;
}
```

## What is being built

Six changes, of which the macro is the smallest. They are one spec because the
macro is unwritable without the other five: a `main` that takes no arguments
cannot set a rate, cannot pick a seat, cannot open a socket and cannot decide
who plays the seats nobody is in.

---

### 1. `Arguments` is replaced

```rust
/// What the run opens on. One field, so two of them is a parse error.
pub enum Load {
    /// A level reference as a JSON-encoded string.
    Level(String),
    /// A save slot.
    Save(SaveSlot),
    /// A recorded session to carry on.
    Demo(PathBuf),
}

#[non_exhaustive]
pub struct Arguments {
    /// Open no window, no adapter and no audio device.
    pub headless: bool,
    /// Claim no seat: submit nothing, watch what the others do.
    pub spectator: bool,
    /// How many unclaimed seats the `Agent` should play.
    pub num_bots: u16,
    /// Stop once this many ticks have run, counted from where the run opened.
    pub ticks: Option<Ticks>,
    /// What to open on, rather than the game's own opening.
    pub load: Option<Load>,
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
| `--state DIR` | `state` |
| `--seat N` | `seat` |
| `--listen PORT` | `listen` |
| `--connect ADDR` | `connect` |

**`Load` is one field on purpose.** Today `load` and `replay` are two fields and
`App::open` resolves the conflict by a documented precedence — "a slot is the
more specific of the two". A precedence rule is a rule nobody reads. Two of
these on one command line is now `Argument::Conflicting { flags: [_, _] }` and
the operator is told which two.

**`Load::Level(String)`** is new. The string is JSON for
`<S::Level as Level>::Reference`, deserialized by the runtime and used in place
of the opening's own level. It is a string rather than anything typed because
the command line has no types, and the deserialization failure is
`Argument::NotALevel { value, why }`.

**`--capture` and `--retain` leave the command line.** `App::capture` and
`App::retain` stay exactly as they are, so `examples/pong/tests/drawn.rs` and the
five `corvid_app` test files that build their own `App` are untouched. What goes
is the ability to record an arbitrary run from a shell. Recording becomes
something a harness does, which is what every existing caller already is.

**`state` replaces `saves`.** Today three call sites resolve a directory
separately: `Saves::resolve` for slots, `Settings::load` for `setting.json`, and
`controls::resolve` for `binding.json`. One field, one directory, one resolution.
`App::saves` becomes `App::state` and the fallback chain is unchanged.

**`Ticks(u64)`** is a new newtype in `corvid_time`, beside `Tick` and `TickSpan`.
A count of ticks and a point in time are different things and `--ticks` means the
first; the existing `u64` says neither.

---

### 2. A seat becomes optional

`--spectator` means this client claims no seat. `App::seat` keeps its
`PlayerId` signature and gains a sibling, `App::spectating()`, which clears it;
the field behind both becomes `Option<PlayerId>`. A sibling rather than widening
`seat` to an `Option`, because `seat` is called by every test and harness in the
workspace and `.seat(Some(PlayerId(0)))` is worse at every one of them.

The runtime grows the absent-seat path:

- `prepare` skips the `Error::Seat` roster check, since there is no seat to
  check.
- The tick loop writes no action into the log for this client. The seat's column
  is filled by a peer or a bot, or stays `Action::default()`.
- With a transport, the peer joins as a listener: it folds in what arrives,
  compares digests, and sends no actions of its own.

This is the deepest change in the spec and the one most likely to surface
assumptions. `Plan.seat` becoming an `Option` is the mechanical part; the loop
in `runtime.rs` assuming it always has somewhere to write is the part to be
careful with.

---

### 3. `Controller` is told which seat it is answering for

```rust
fn action(&self, state: &S, input: &Input, time: Time, seat: PlayerId) -> S::Action;

fn update(
    &mut self,
    state: &S,
    input: &Input,
    loading: Option<Loading<'_, LevelRef<S>>>,
    time: Time,
    dt: Duration,
    seat: PlayerId,
);
```

An explicit parameter rather than a field on `Time`, because `Time` is also
handed to the simulation and a seat is not something a tick may read.

One `Agent` instance answers for every bot seat: the runtime calls `action` once
per bot seat with that seat's number, and a bot that plays differently depending
on which end of the court it defends reads its argument. The alternative — one
instance per seat, built from a per-seat config — would need the runtime to
manufacture a `Config` it cannot know the shape of.

`impl Controller<S> for ()` gains the parameter and ignores it. Every other
implementation in the workspace gains an ignored parameter except pong's `Hands`
and the new `Bot`.

---

### 4. `App` gains the Agent

```rust
pub struct App<S: State, C = (), R = (), A = (), B = ()>
where
    C: Controller<S>,
    R: Render<S>,
    A: Auralizer<S>,
    B: Controller<S>,
{ … }
```

`B` is the Agent: a second `Controller<S>`, defaulting to `()`, which already
declares no actions, wants no devices and submits the idle action forever. A game
with no bots writes nothing.

**Which seats bots play.** Unclaimed roster seats in order — skipping this
client's seat when it has one — up to `num_bots`. In pong, `--seat 0 --bots 1`
gives the bot seat 1; `--spectator --bots 2` gives it both.

**`--bots` with `--connect` is refused**, as `Argument::Conflicting`. Two peers
each running their own bots would each write the same seat's column, from
controllers that are not hashed and need not agree. The refusal is at the command
line rather than at the first divergence.

Bot actions are written into the log the same way this client's are, so they are
part of the session, recorded by a capture and replayed by a `--demo`.

---

### 5. `State::RATE`

```rust
pub trait State: Default + Data {
    /// How often a tick runs.
    const RATE: TickSpan = TickSpan::CRADLE;
    …
}
```

The macro has nowhere to write `.rate(RATE)`, and a tick rate is a property of
the game rather than of the run: two peers at different rates are two different
sessions. pong writes `const RATE: TickSpan = crate::RATE;` and `App::rate`
stays for a harness that wants to run a game fast.

---

### 6. The macro

```rust
corvid::app! {
    type State = Table;
    type Controller = Hands;
    type Agent = Bot;
    type Render = Graphics;
    type Auralizer = Ears;
}
```

expands to

```rust
fn main() -> corvid::Result {
    corvid_app::main::<Table, Hands, Bot, Graphics, Ears>()
}
```

Every line but `State` is optional and defaults to `()`; lines may appear in any
order. The macro does nothing a function call cannot — it exists because five
positional type parameters are unreadable, and because a game naming its types by
role is the documentation of what those five slots are.

`corvid_app::main` gains the four parameters and does, in order:

1. `watch()`.
2. Parse. `--help` writes the usage to stdout and answers `Ok(())`.
3. Build the `App`: `S::opening()`, `S::RATE`, `Input::new(C::SETS)`, the seat or
   the absence of one, the number of bots, and `C::bindings()` for a windowed run.
4. Open the transport from `--listen`/`--connect`, when both are given.
5. Pick the backend — **not** under `cfg` as today, but from what the run is
   doing: a window when one was not refused and the build has the feature; an
   adapter drawing offscreen when there is a capture and no window; nothing
   otherwise. This is pong's three-way choice, promoted.
6. Run.
7. Report.

**Reporting.** Everything the framework has to say goes through `tracing`: one
`corvid_app.finished` event carrying the ending tick, the settled tick, the
digest, the `Traffic` counters and the request count. A headless run also writes
the settled digest — and nothing else — to stdout, because that is the program's
answer to a pipe rather than an event.

The **settled** digest, not the last tick's: on a networked run the newest ticks
were simulated partly from predictions, so two peers that stopped a second apart
print different numbers for the same session. pong works this out today with a
`SETTLED: u64 = 20` constant; the reasoning is about the netcode's budget rather
than about pong, and it moves into `corvid_app` beside the budget it is derived
from.

---

## What pong becomes

`main.rs` is the macro invocation and its module documentation. Everything else
in it is deleted: the `Ours` struct, the parser, the usage text, `socket`,
`socket_error`, `demo`, `together`, `halted`, `report`, `netcode`, `SETTLED`,
`OFFSCREEN`, `TICKS`.

`--demo` and `--together` **go away entirely.** `rally::Match`, `rally::together`
and `rally::Policy` stay as library code, and they are already driven by
`tests/linked.rs` and `tests/together.rs` — so the netcode is exhibited by the
test suite, which is measured, rather than by a flag, which is a demo somebody
has to run.

`Hands::scripted` and its `Config = Option<u16>` go away. A scripted paddle is
the Agent's job now, and `Hands::Config` becomes `()`.

`Bot` becomes a real type in `bot.rs` implementing `Controller<Table>` with
`REAL = false` and `SETS = &[]`, wrapping the existing `target` and `toward`
functions and reading its seat from `action`'s new parameter. `rally::Racket`
collapses onto it.

pong's bespoke scoreline is deleted along with `report`. A game that wants to
say something about its own state emits a `tracing` event from its own
client-local code — `Extract::extract` sees every state — rather than through a
hook on `State`, which is otherwise purely the simulation.

## Testing

- `corvid_app/tests/arguments.rs` is rewritten against the new surface: every
  flag, the two-`Load` conflict, `--bots` with `--connect`, and a malformed
  `--level`.
- A new test that `--spectator` runs a session, writes no action for this client,
  and reaches the same digest as a run whose seat submitted `Action::default()`.
- A new test that `--bots N` fills the expected seats and that the bot's actions
  are in the recorded session.
- `corvid_app/tests/headless.rs`, `capture.rs`, `retention.rs`, `saves.rs`,
  `commands.rs` build their own `App` and should compile unchanged apart from
  `saves` → `state` and the `Controller` signature.
- pong's eight test files: `session.rs`, `socket.rs`, `bot.rs` and `baseline.rs`
  touch the deleted `Hands::scripted` and move to `Bot`.
- The determinism claim that matters: a run with `--bots 1` and a run whose
  second seat was driven by the same `Bot` through a hand-built `App` produce
  identical traces.

## Risks

**The absent seat.** `runtime.rs` and `corvid_lockstep` assume a seat to write
into. If the listener path turns out to need changes inside `Peer`, that is a
larger change than this spec accounts for and is the first thing to find out.

**Signature churn.** Adding a parameter to `Controller::action` and `update`
touches every implementation in the workspace and in `examples/`. Mechanical,
but wide.

**Losing `--capture` from the CLI** means no shell can record a run. Every
current caller builds its own `App`, so nothing breaks today; a future golden
harness would have to build one too.
