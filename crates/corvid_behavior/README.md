# `corvid_behavior`

The deterministic half of a Corvid game: three data types and two functions, on
the state itself.

Everything a peer, a save, a replay or a digest ever sees is here. Nothing here
knows what a window, a device or a frame rate is — a dedicated server, a
determinism check in CI and a game's own `cargo test` link this crate and stop.

```rust
use corvid_behavior::{Command, Level, Player, State};
use corvid_files::{Malformed, Source};
use serde::{Deserialize, Serialize};

/// Authored, immutable within a session, read off a `Source`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Field {
    width: u16,
}

impl Level for Field {
    /// How this game names a level. An enum for a fixed set, a path for one
    /// that loads from disk; this one uses a string.
    type Reference = String;

    fn load(reference: &String, files: &dyn Source) -> Result<Self, Malformed> {
        let bytes = files.read(reference)?;
        let width = u16::from(
            *bytes
                .first()
                .ok_or_else(|| Malformed::at(reference, "a field needs a width"))?,
        );
        Ok(Self { width })
    }
}

/// Everything that cannot be recomputed: serialized, hashed, rolled back.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Walk {
    at: u16,
}

/// One player's intent for one tick. Goes on the wire; `Default` is idle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Step(bool);

impl State for Walk {
    const NAME: &'static str = "walk";
    type Rules = ();
    type Level = Field;
    type Action = Step;

    fn tick(
        self,
        level: &Field,
        players: &[Player<'_, Step>],
        _rules: &(),
        command: &mut impl Command<Reference = String>,
    ) -> Self {
        let stepped = players.iter().filter(|player| player.action.0).count();
        let at = self
            .at
            .saturating_add(u16::try_from(stepped).unwrap_or(u16::MAX))
            .min(level.width);
        if at == level.width {
            command.quit(corvid_behavior::ExitCode::SUCCESS);
        }
        Self { at }
    }
}
```

That is a whole game. It states three types and a name, and writes one function.

## Implemented by the state, not by a marker

This crate used to require a marker type carrying five associated types, and the
reason given was the orphan rule: an art crate could not implement a Corvid
trait for a simulation crate's type.

That reason is gone. A renderer no longer implements anything *for* the state —
it implements `Extract<S>` for **its own** type, which its own crate owns, and
the state is a type parameter. So the marker went, and what is left is a trait
you put on the struct you were already writing.

## The three types

| | What it is |
|---|---|
| `Level` | Authored, immutable within a session. Read off a `Source`, never inside a tick. |
| `Rules` | Deterministic tuning every peer must agree on. Feeds the hash. |
| `Action` | One player's intent for one tick. Goes on the wire. `Default` is idle. |

The state is `Self`. There is no fourth type for it, and no fifth for a scratch.

## What became of `Scratch`

It is gone. It was a memo channel into the tick, carrying an obligation — *a
memo, never an accumulator* — that no type could state and that a rollback could
silently violate. A scratch's value at tick N was a function of every tick
before it, and the runtime does not preserve that chain: a seek restores a
snapshot and re-simulates with whatever scratch its caller has, a rollback
re-runs ticks the scratch has already been through once, and the snapshot ring
is sized by one machine's spare memory, so *which* states survive to be resumed
from is a property of that machine rather than of the session.

What replaces it is `self` by value. A tick that wants to reuse an allocation
takes the `Vec` out of the state it was handed and puts it in the state it
returns — the same move, without a channel that was invisible to the hash.

## A round trip has to give back what went in

`Data` is the bundle of what a simulation's values owe: `Serialize`,
`DeserializeOwned`, `Hash`, `Eq`, `Clone`, `Debug`. It is blanket-implemented,
so no type ever names it, and it carries two obligations no bound can state.

**A round trip has to be faithful.** A `Serialize` that skips a field its
`Deserialize` expects, a `#[serde(skip)]` on something the state needs, a
`#[serde(into = "…")]` whose conversion loses precision — these are one bug in
different clothes. The tick that produced the state is deterministic in every
case, and the game still comes apart, because the state that arrives is not the
state that left.

```rust
use corvid_behavior::round_trip_is_faithful;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Score {
    points: u32,
}

assert!(round_trip_is_faithful(&Score { points: 7 }).is_ok());
```

Point it at the states a session actually reaches, and **not** at
`State::default()` — that is the value a lost field decays to, and so the value
most likely to survive anything.

**A clone has to be a copy.** `Clone` must give back a value that is `Eq` to its
source and digests the same. The derive does this; a hand-written `Clone` that
reseeds a field, resets a counter or drops a cache does not — and a snapshot
taken by cloning a state is only a snapshot if the clone is one.

## `tick` takes `self` and returns `Self`

There is no `&mut self` to accumulate into and no extra argument to hide state
in, so everything the tick is allowed to know is named in its signature.

That is a narrowing and not a proof. A method can still call `Instant::now()`,
read an environment variable, or load a `static AtomicU64` that was set at
startup, and no signature stops any of it. Keeping a simulation crate `no_std`
is a second narrowing and not a proof either: it puts the clock, the environment
and the filesystem out of easy reach, and a `no_std` crate that writes
`extern crate std` has them all back.

A process-global with interior mutability survives both narrowings, and it is
the one leak no check inside a single process can find. What finds it is two
peers that are genuinely two processes comparing digests.

### Values, not identities

*Read only your arguments* is the rule a reader infers from the signature, and it
is too weak. `Arc::strong_count(level)` reads nothing but an argument and is
still peer-local: a peer whose runtime holds a second handle — a deeper snapshot
ring, a spectator feed, a recording — counts one more from the same level
*value*. `Arc::as_ptr`, `players.as_ptr()`, and any ordering derived from an
address are the same hole.

So the obligation is the stricter one: **read only the values these arguments
denote.** A level's contents, never its handle.

## Commands are a sink, not a return value

`Command` is a trait with one method per effect, and a tick is handed a
`&mut impl Command`. Every method has a default that does nothing.

```rust
use corvid_behavior::{Command, ExitCode, SaveSlot};

/// A sink a test can assert on.
#[derive(Debug, Default, PartialEq, Eq)]
struct Recorder {
    quits: Vec<ExitCode>,
    saves: Vec<SaveSlot>,
}

impl Command for Recorder {
    type Reference = String;

    fn quit(&mut self, code: ExitCode) {
        self.quits.push(code);
    }

    fn save(&mut self, slot: SaveSlot) {
        self.saves.push(slot);
    }
}

let mut sink = Recorder::default();
sink.quit(ExitCode::SUCCESS);
sink.achieve(corvid_behavior::AchievementId(3)); // no code for it; dropped
assert_eq!(sink.quits, [ExitCode::SUCCESS]);
```

Two things came of the change. **A tick that asks for nothing allocates
nothing**, which is almost all of them — the old shape returned
`Vec<Command<R>>`, and every element of every non-empty one was as wide as the
widest request, which is why the big payloads were boxed. And **a test can be a
`Vec`**, which is the reason the shape changed at all.

A sink that wants none of it can be `Discard<R>`.

The scope of each request — whether it is about the session or about one machine
— is written on each method. It used to be an accessor, because a match on a
`#[non_exhaustive]` enum is forced to write a fallback arm holding an unknown
request with no way to ask what kind it is. One method per effect makes that
problem disappear rather than solving it.

## Exactly one action per player, always

A tick is handed a slice of `Player`, each carrying exactly one `Action`. A
player who did nothing submits `Action::default()`, and a dropped player submits
it forever — so a game never asks whether an action is present, and there is no
`Option` to get wrong.

Identity comes from the runtime, in `Player::id`, and never from anything the
game can read off an action. A `Presence` says whether the seat is joining,
active or dropped, and it hashes alongside the state.

### There is no pose on a `Player`

A headset's pose is client-local and arrives through the controller, not here.
What crosses into the simulation is an `Action` the game named — never a
number a device produced.

## Names, not indices

`ExitCode`, `SaveSlot`, `RumbleId`, `AchievementId`, `StatId` and `LobbyId` are
newtypes, so a rumble effect cannot be passed where an achievement was meant.
`PresenceText` and `Url` are bounded at 64 and 256 bytes: the bound buys a fixed
encoding, so the line digests identically on a peer whose platform would have
truncated it somewhere else, and a refusal at the boundary rather than a quietly
shorter line on one machine.

## `Time`, `Loading` and `Extract` live here, and the client traits do not

`Time` is where the session is — a tick and a wall clock. `Loading` is how far
along one machine's bytes are. `Extract` is state into whatever a device wants.

All three are named by more than one crate above this one, which is why they are
here rather than beside the traits that use them: putting `Loading` next to
`Controller` would make the renderer depend on the controller's crate for a
struct with two fields.

What is **not** here is `Controller`, `Render` or `Auralizer`. The line between
the deterministic half and the client half is this crate's edge, and it is real:
nothing above it can be read by a tick, and nothing below it knows a display
exists.

## The two wire formats

Everything here has two encodings that share nothing — a hand-written `Hash` and
a derived `Serialize` — and `tests/golden.rs` and `tests/wire.rs` freeze both.
Two peers on different builds compare digests, so a field that changed its
hashing order is a desync, and a field that changed its serialized form is a
save that will not load.

`Digestible` implementations are hand-written throughout, never derived: the
derive links `syn`, and this crate is on the path of every build.
