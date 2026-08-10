# `corvid_replay`

Save, load, replay, rollback and time-walk are one function in
[Corvid](https://github.com/peasanttide/corvid), and this crate is that
function and the three things it reads. A [`Session`] is an [`Opening`], an
[`ActionLog`] and a [`HashTrace`]; [`Session::seek`] restores the nearest
snapshot at or before a tick and re-simulates forward against the log. A save
writes the session down. A load is a seek. A rollback is a seek after a
correction. A slider is a seek per frame. There is no second code path, which is
why a bug in one of them is a bug in all of them and shows up in whichever gets
tested.

Everything here is `no_std`. It depends on the `Simulate` contract, the digest,
`corvid_wire`, and `corvid_time` for `Tick` — and it does not forward that
crate's `std` feature, which is the one that adds a wall clock. A replay is the
part of the stack that has no business asking what time it is.

```rust
use std::sync::Arc;

use corvid_behavior::PlayerId;
use corvid_hash::digest;
use corvid_replay::{ActionLog, HashTrace, Opening, Profile, Schema, Seed, Session, Snapshots};
use corvid_time::Tick;
# use corvid_behavior::{Command, Level as LevelContract, PlayerState, State};
# use serde::{Deserialize, Serialize};
#
# #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
# struct Level { ceiling: i64 }
# impl LevelContract for Level {
#     type Error = core::convert::Infallible;
#     fn load(_: &str) -> Result<Self, core::convert::Infallible> { Ok(Self { ceiling: 100 }) }
# }
# #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
# struct Rules { step: i64 }
# #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
# struct Counter { count: i64 }
# #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
# enum Action { #[default] Idle, Bump }
# impl State for Counter {
#     const NAME: &'static str = "counter";
#     type Level = Level;
#     type Rules = Rules;
#     type Action = Action;
#     fn tick(
#         self,
#         level: &Level,
#         players: &[PlayerState<Action>],
#         rules: &Rules,
#         _command: &mut impl Command,
#     ) -> Self {
#         let bumps = players.iter().filter(|p| matches!(p.action, Action::Bump)).count() as i64;
#         Self { count: (self.count + bumps * rules.step).min(level.ceiling) }
#     }
# }

// The description of this build's types. Two builds that describe themselves
// differently refuse each other's captures rather than diverging.
let schema = Schema::new("counter").field("Counter.count", "i64").digest();

let opening = Opening::<Counter> {
    level: "terminus".to_owned(),
    content: Arc::new(Level { ceiling: 100 }),
    rules: Arc::new(Rules { step: 3 }),
    roster: vec![Profile { account: corvid_behavior::ProfileId(7), joined: Tick::ZERO, left: None }],
    seed: Seed(0x5eed),
    first: Tick::ZERO,
    // `None` would be `Counter::default()`, which is this same state.
    origin: Some(Arc::new(Counter { count: 0 })),
    schema,
};

// The one thing this refuses is an opening whose roster is wider than a
// `PlayerId` can address, because the log it builds is as wide as the roster.
let mut session = Session::new(opening)?;

// Four ticks, of which the first and third are bumps. Growing the log is a
// separate call from writing to it, and the next section says why.
session.log.extend_to(Tick(3))?;
session.log.set(Tick(0), PlayerId(0), Action::Bump)?;
session.log.set(Tick(2), PlayerId(0), Action::Bump)?;

// The state comes back behind a handle, because that is how everything that
// asks for one holds it: a runtime keeps its two states that way so it can put
// them in a `Frame` without copying either.
let mut snapshots = Snapshots::new(64 * 1024);
let (state, replayed) = session.seek(&mut snapshots, Tick(4))?;
assert_eq!(state.count, 6);

// The ring is a cache. Emptying it changes what the seek costs and not what it
// returns — and the cost is the second half of the answer, because nothing
// about the state itself could ever say whether the ring was consulted.
let mut cold = Snapshots::new(0);
let (again, from_cold) = session.seek(&mut cold, Tick(4))?;
assert_eq!(digest(&state), digest(&again));
assert!(from_cold >= replayed);

// A capture recorded by a build describing itself differently is refused
// rather than replayed.
let bytes = session.save()?;
let elsewhere = Schema::new("counter").field("State.count", "i128").digest();
assert!(matches!(
    Session::<Counter>::load(&bytes, elsewhere),
    Err(corvid_replay::Load::Schema { .. }),
));
assert!(Session::<Counter>::load(&bytes, schema).is_ok());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## The log is dense, and absent is idle

One action per seat per tick, flat, at `(tick - first) * players + player`. An
entry nobody wrote holds `Action::default()`, which is what "this player did
nothing" already means in `corvid_behavior`, so a replay never asks whether an
action is present any more than a game does.

| | Where it lives |
|---|---|
| One seat's action for one tick | [`ActionLog::get`] |
| Every seat's action for one tick | [`ActionLog::row`] |
| How far the log reaches | [`ActionLog::last`] |
| Whether anybody confirmed an entry | [`ActionLog::is_confirmed`] |

A bit per entry records whether anybody has written it, and that bit is not the
same question as whether the entry differs from the default. A peer that
confirmed an idle action and then sends something else for the same tick is
contradicting itself, and a log that compared against the default rather than
reading the bit would accept the contradiction — a confirmed idle and an absent
entry are the same bytes.

```rust
# use corvid_behavior::PlayerId;
# use corvid_replay::{ActionLog, Refused};
# use corvid_time::Tick;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Action { #[default] Idle, Bump }

let mut log: ActionLog<Action> = ActionLog::new(Tick::ZERO, 2);
log.extend_to(Tick(0))?;

// A packet that arrives twice changes nothing and is not an error.
log.set(Tick(0), PlayerId(0), Action::Bump)?;
log.set(Tick(0), PlayerId(0), Action::Bump)?;

// A packet that contradicts what the session already simulated is refused.
assert_eq!(
    log.set(Tick(0), PlayerId(0), Action::Idle),
    Err(Refused::Confirmed { tick: Tick(0), player: PlayerId(0) }),
);

// And the case the bit is for: seat 1 confirmed the *default*, which is the
// same bytes as never having been written, and is still confirmed.
log.set(Tick(0), PlayerId(1), Action::Idle)?;
assert!(log.is_confirmed(Tick(0), PlayerId(1)));
assert_eq!(
    log.set(Tick(0), PlayerId(1), Action::Bump),
    Err(Refused::Confirmed { tick: Tick(0), player: PlayerId(1) }),
);
# Ok::<(), Refused>(())
```

That pair — idempotent for the same value, refused for a different one — is how
a rollback tells a correction from a duplicate. It is also why growing the log
is [`ActionLog::extend_to`] and not something `set` does on demand. A tick
number is the one thing in a session that arrives from somewhere else, and a
`set` that grew to fit would turn `Tick(u64::MAX)` in a packet into a request
for sixteen exabytes. Growing is a decision made at a call site that knows how
far ahead of the present it is willing to record, and the room is asked for with
`try_reserve` so that a request this machine cannot meet is
[`Refused::Memory`] rather than an abort.

## The ring is sized by bytes, and keeps a spread

A tick count is the wrong unit for a snapshot ring because it does not say what
it costs: fifty thousand entities is about a megabyte of state, and two hundred
of those is two hundred megabytes on a machine that was told to keep two hundred
ticks. [`Snapshots::new`] takes the number the operator actually has.

What a state is charged is the length `corvid_wire` writes it as, plus the ring's
own entry for it. That is an estimate rather than a measurement: a `Vec` inside a
state with room for a thousand rows and three rows in it is charged for three, so
a ring asked for sixty-four mebibytes may hold rather more than that. Charging
honestly would mean asking every state how much memory it owns, which is a
method `Simulate` does not have and one more thing a game could get wrong.
Measuring by encoding costs a serialization per snapshot kept, which is the price
and is worth knowing before keeping one every tick.

Eviction thins the snapshots between the oldest and the newest and leaves those
two alone, in favour of recent ticks — so the gaps grow with age. A ring that
kept only the newest states would be perfect for a rollback of six ticks and
useless for a slider dragged into the middle of an hour-long session, and those
are the same function with a different argument, so the ring cannot be tuned for
one of them. When there is nothing left between the two to thin, the oldest is
what goes: it is the one the opening can stand in for.

None of that changes an answer, as long as every state in the ring was simulated
against the log being seeked: `tests/seek.rs` runs one session at a budget of one
snapshot and of a hundred and compares the state at every tick against a forward
run. Keeping that qualification true across a correction is the caller's job, and
the next section is that job.

## `seek` re-simulates from the log and nothing else

[`Session::seek`] restores a snapshot and runs forward, and there is nothing a
tick can carry across that boundary: `State` has no scratch associated type, so
a tick reads `previous`, `level`, `players` and `rules` and nothing besides.
A scratch channel would need an obligation beside it — that whatever a tick read
out of it be a pure function of those same four values — and no type could state
that obligation or catch a tick breaking it.

The property that makes it matter is this. Which snapshot a seek
starts from is a property of one machine's memory budget and of where a player
dragged a slider, and so is how many ticks it then re-simulates and which
states the ring lets go of on the way. A tick that accumulated anything at all
would therefore replay to a different state than it ran, from the same log, on
the same machine, with nothing on the wire to blame — which is the reason the
associated type that allowed it is not there any more.

## A correction invalidates the snapshots after it, and the log says which

A snapshot is a tick and a state, and a state alone does not say which history
it came from. So the moment [`ActionLog::set`] takes a correction for tick `T`,
every snapshot *after* `T` is a state of a history that did not happen — the row
at `T` carries the state at `T` to the state at `T + 1`, so the state at `T`
itself survives — and a seek that landed on one of them would return it without
re-simulating, leaving the correction in the log and out of the answer.

The log is what settles it. A write that *changes* a stored action is counted,
and [`ActionLog::generation_at`] reports how many of those have landed on rows
strictly before a tick — which is exactly the set of rows the state at that tick
was built from. [`Snapshots::keep`] records that number and [`Snapshots::nearest`]
skips an entry whose number the log no longer agrees with, so a rollback lands on
the newest snapshot the correction did *not* invalidate and re-simulates from
there.

Both halves of that matter and the second is the one worth arguing for, because
"strictly before" reads like an off-by-one and is the whole rule. The state at
`T` is what simulating the rows at `first` through `T - 1` produces; the row at
`T` is what carries it on to `T + 1`. So the state at `T` does not depend on row
`T`, a snapshot at `T` is keyed to the rows before `T`, and a snapshot at the
opening is keyed to none of them.

The looser rule — count the row at `T` too — looks like a safe
over-approximation and is not one. An over-approximation throws away some
entries that were still good and keeps the rest. This one keeps nothing:
ordinary forward play keeps the state at `S` and only then learns what the seats
did on `S`, so writing row `S` would invalidate the snapshot taken at `S` a
moment earlier, on every tick, for every entry, in exactly the case the ring
exists to serve. What it approximates is not a smaller ring but an empty one
with the bookkeeping still paid for, and every seek back to the opening.
`tests/seek.rs` runs that case and `tests/log.rs` pins the boundary entry by
entry.

Two things the generation does not do. A skipped entry is still charged against
the budget, so [`Snapshots::discard_from`] is still worth calling after a
rollback — it is what gives that budget back, and it is the counterpart of
[`HashTrace::truncate_from`]. And a [`Session::log`] *replaced*
wholesale rather than corrected shares no history with the one the ring was
filled from; nothing can compare them, and that case is [`Snapshots::clear`].

The generation is not written down and not compared for equality. It is about
one machine's ring rather than about the session: two peers holding identical
actions took them in a different order, and a capture that is loaded has no ring
to be stale against.

## A session can forget its own beginning

A session played forward grows a row of actions and a mark every tick and reads
neither of them again. Nothing in the three types above has an opinion about how
long that is worth doing, and for a session being recorded the answer is
forever; for a game somebody has left running it is not. So the log and the
trace can each let go of a prefix, and [`Session::forget_before`] is the call
that moves all three parts at once — the log, the trace, and the opening, which
has to arrive at the new first tick with the state that belongs there or the
session is one [`Session::check`] refuses.

```rust
# use corvid_behavior::PlayerId;
# use corvid_hash::Digest;
# use corvid_replay::{ActionLog, HashTrace, Refused};
# use corvid_time::Tick;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Action { #[default] Idle, Bump }

let mut log: ActionLog<Action> = ActionLog::new(Tick::ZERO, 2);
let mut marks = HashTrace::new(Tick::ZERO);
log.extend_to(Tick(99))?;
for tick in 0..100 {
    log.set(Tick(tick), PlayerId(0), Action::Bump)?;
    marks.push(Digest::from_u64(tick));
}

log.forget_before(Tick(90));
marks.forget_before(Tick(90));

// The frontier does not move: what changed is how far back the session
// reaches, and a runtime writing at the end writes exactly where it did.
assert_eq!(log.last(), Tick(100));
assert_eq!(marks.end(), Tick(100));
assert_eq!(log.ticks(), 10);

// What is left reads as it always did, bit for bit — including the
// confirmation bits, which move by entries rather than by bytes.
assert_eq!(log.get(Tick(95), PlayerId(0)), Some(&Action::Bump));
assert!(log.is_confirmed(Tick(95), PlayerId(0)));
assert!(!log.is_confirmed(Tick(95), PlayerId(1)));

// And what is gone is gone, rather than wrong.
assert_eq!(log.get(Tick(89), PlayerId(0)), None);
assert_eq!(marks.get(Tick(89)), None);
# Ok::<(), Refused>(())
```

**This costs reach and nothing else.** Save, replay, rollback and time-walk are
the same one function over whatever the session still holds, so a session that
kept the last few hundred ticks can do all four across them and none of them
before that. Those four are still one function; the window it sees through is
shorter.

Two things a caller owes. The state handed over has to be the state at that
tick — nothing here can recompute it, since recomputing it is exactly what the
rows that have just gone were for — and a [`Snapshots`] ring alongside it has to
be told, because the entries at and before the new first tick are keyed to a
history this session no longer has. [`ActionLog::forget_before`] is exact about
which direction that comparison fails in.

It is handed over as an `Arc`, which is how a runtime is already holding the
state it is displaying, so the one call whose purpose is to stop holding memory
does not begin by copying a whole state. [`Opening::origin`], [`Opening::rules`]
and [`Opening::content`] are all handles for that reason: each of them is a
value a `Frame` carries, several times a second, and none of them is ever
mutated in place. The capture does not know — the wire format writes the values
and not the handles, and [`Opening::schema`] and every mark are exactly the
digests they were.

## What a replay does not reproduce

**Nothing about the inputs, which is the reason a `Player` has three fields.**
Every one of them comes back out of the capture: the seat is the roster's order,
the presence is [`Profile::presence_at`] over its join and leave ticks, and the
action is the log's. This crate is what rules a head-and-hands pose out of that
struct — there would be nothing here to rebuild one from, so every player would
be handed the identity and a game that read it would replay to a different state
than it ran, silently. The rule is that every
input a tick can see has to be in the log; a game that wants poses puts them in
its own `Action`, where the log carries them already. `tests/seek.rs` names each
of the three fields exhaustively, so a fourth stops compiling rather than
quietly reopening the gap.

**Commands.** The requests a re-simulated tick returns are dropped. They were
made when those ticks first ran, and a replay that re-issued them would save a
file, take a screenshot, or quit a second time.

**A `Session` put out of step by hand.** The three fields are public because a
lockstep transport writes the log, a desync check reads the marks and a dev
console reads the opening, and the cost is that they can be made to disagree: a
seat the log has no column for reads `Action::default()` and a row wider than the
roster has its extra columns ignored, which replays a session that never happened
rather than failing.

[`Session::check`] is the comparison, and [`Session::load`] makes it on every
capture, with [`Load::Shape`] naming which two parts disagree. There are six
ways to disagree — the log or the trace starting at a different tick than the
opening, a roster with more seats than a [`PlayerId`] can address, rows a
different width than the roster, entries that stop partway through a row, and a
confirmation bitmap a different length than the entries need.

The last two are the ones that do not announce themselves. A bit past the end of
the bitmap reads as zero, so a capture a byte short arrives with entries the
recording peer had agreed on reading as unconfirmed, and every one of them can
then be rewritten with no refusal anywhere: the log losing its authority, and it
decodes perfectly. A partial row is quiet for the opposite reason. Nothing can
reach it — [`ActionLog::ticks`] counts whole rows — right up until the session
records one more tick, at which point those entries are the front of the new row
and arrive holding actions the capture never recorded, confirmed. From there the
seats they belong to are refused when they send the real ones.

`check` is public rather than folded into a constructor because there is nothing
left for a constructor to do. [`Session::new`] builds the log and the trace
*from* the opening, and refuses the one opening it cannot build them from: a
roster wider than a `PlayerId` can address has no width the log could take, and
saturating it would be a constructor handing back a session `check` refuses.
Every other disagreement arrives afterwards, through a `pub` field, and an
assignment to one of those is not something a type can observe.

## The schema is a string somebody wrote

[`Opening::schema`] is compared at [`Session::load`] and a mismatch is
[`Load::Schema`]. [`Schema`] is how the digest is produced, and what it is
worth is narrower than the name suggests: Rust has no reflection this crate
could use, so it hashes a description a person wrote and nothing checks the
description against the types. A field added to a `State` and not added to the
description leaves the digest exactly where it was, both builds load each
other's captures, and the divergence happens anyway.

What it does buy is that two builds which *describe themselves differently* are
told apart at load rather than at the first tick where their states differ. The
habit that makes it work is editing the description in the same commit as the
type.

## Two golden tables, because neither sees the whole encoding

A `Session` is written down with `corvid_wire`, so `tests/wire.rs` records what
every type here encodes to as hex literals and `tests/names.rs` records what the
same fixtures write under a self-describing format. Neither substitutes for the
other. The byte table is the only thing that can see an integer widen — a width
is a length there and nowhere else — and it writes no names, so it cannot see a
field renamed, two same-typed fields exchanged when the fixture holds the same
value in both, or a field added that encodes to nothing. The name table sees
exactly those three, unconditionally, and cannot see a width at all.

`tests/wire.rs` and `tests/names.rs` each carry the probe that shows they are
worth their place: a fixture written down under a second declaration that
differs by exactly one change, asserted to encode differently. A table that only
catches value changes is decoration.

## Features

| Feature | Effect |
|---|---|
| `std` | Forwards `std` to the crates below. Adds no API. |

`corvid_time/std` is deliberately not forwarded, exactly as it is not from
`corvid_behavior`: the API that feature adds is `Wall`, a clock, and Cargo
unifies features across a build. A replay is the part of the stack that must not
be able to ask what time it is.

## Tests

```sh
cargo test -p corvid_replay --all-features
```

| File | Covers |
|---|---|
| `tests/log.rs` | Dense indexing, the confirmed bit, idempotence, the refusals, that growing is separate from writing and leaves what it grew unconfirmed, and what counts as a correction |
| `tests/trace.rs` | Marks, truncation on rollback, and what `disagrees_with` compares and does not |
| `tests/forget.rs` | That forgetting a prefix leaves every retained entry, confirmation bit, correction count and mark exactly where it was, that the frontier does not move, that what is left seeks and saves and loads, and that the two ticks outside the session are refused by name |
| `tests/seek.rs` | That a seek reaches what a hand-written forward run reached, at every budget; that a rollback recovers the forward result, that a stale snapshot is not returned and that ordinary play does not go stale; presence; that a seat no `PlayerId` can name is left out rather than folded onto the last; and that every field of a `Player` is one the capture can rebuild |
| `tests/snapshots.rs` | What the ring charges, what it replaces, what it discards, what a state too large for the budget does, and that the thinning favours recent ticks |
| `tests/roundtrip.rs` | That a session survives being written down, that a capture from an incompatible build or with parts that disagree is refused by name, and what a short confirmation bitmap and a partial row would each have cost |
| `tests/wire.rs` | Every type's bytes, frozen as literals, with the probe that a reordered field, a renumbered variant and a widened integer each move them |
| `tests/names.rs` | The same fixtures under a self-describing format, with the probe that a renamed field and a same-typed swap each move it and no byte |
| doctests | Every Rust block in this file |

[`ActionLog`]: crate::ActionLog
[`ActionLog::get`]: crate::ActionLog::get
[`ActionLog::row`]: crate::ActionLog::row
[`ActionLog::last`]: crate::ActionLog::last
[`ActionLog::ticks`]: crate::ActionLog::ticks
[`ActionLog::is_confirmed`]: crate::ActionLog::is_confirmed
[`ActionLog::extend_to`]: crate::ActionLog::extend_to
[`HashTrace`]: crate::HashTrace
[`Load::Schema`]: crate::Load::Schema
[`Load::Shape`]: crate::Load::Shape
[`Opening`]: crate::Opening
[`Opening::schema`]: crate::Opening::schema
[`Opening::origin`]: crate::Opening::origin
[`Opening::rules`]: crate::Opening::rules
[`Opening::content`]: crate::Opening::content
[`Refused::Memory`]: crate::Refused::Memory
[`Schema`]: crate::Schema
[`Session`]: crate::Session
[`Session::load`]: crate::Session::load
[`Session::check`]: crate::Session::check
[`Session::new`]: crate::Session::new
[`PlayerId`]: corvid_behavior::PlayerId
[`Session::seek`]: crate::Session::seek
[`Snapshots::discard_from`]: crate::Snapshots::discard_from
[`HashTrace::truncate_from`]: crate::HashTrace::truncate_from
[`ActionLog::set`]: crate::ActionLog::set
[`ActionLog::generation_at`]: crate::ActionLog::generation_at
[`Snapshots::keep`]: crate::Snapshots::keep
[`Snapshots::nearest`]: crate::Snapshots::nearest
[`Snapshots::clear`]: crate::Snapshots::clear
[`Session::log`]: crate::Session::log
[`Profile::presence_at`]: crate::Profile::presence_at
[`Snapshots::new`]: crate::Snapshots::new
[`Snapshots`]: crate::Snapshots
[`Session::forget_before`]: crate::Session::forget_before
[`ActionLog::forget_before`]: crate::ActionLog::forget_before
