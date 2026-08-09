# `corvid_signal`

Latest-value cells for [Corvid](https://github.com/peasanttide/corvid): how one
subsystem tells another what the world looks like now, across a thread boundary,
without either of them waiting for the other.

A signal holds exactly one value. Publishing replaces it, observing hands back a
shared handle on whatever is there, and everything published between two
observations is dropped and cannot be recovered. That is the whole design, and
the next two sections are about which half of a game it is right for and which
half it would quietly break.

`corvid_app` is what consumes it: the runtime publishes a run's progress
through one of these, and `corvid_window` publishes the surface state the same
way, so a thread reading either never holds a loop up.

```rust
use corvid_signal::{Seen, channel};

/// What the platform layer knows about the window, and the renderer needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Surface { width: u32, height: u32, occluded: bool }

let (published, watch) = channel(
    "surface",
    Surface { width: 1280, height: 720, occluded: false },
);

// A consumer that has seen nothing is told the state that is already there, so
// a subsystem starting up mid-session does not render at the wrong size until
// somebody next drags the window.
let mut seen = Seen::default();
assert_eq!(watch.changed_since(&mut seen).map(|s| s.width), Some(1280));

// What a read hands back is an `Arc<T>` and not a `T`, which is why the
// comparisons below go through it rather than against it.

// The window is dragged across two monitors and then covered up, while the
// consumer is busy drawing a frame.
published.set(Surface { width: 1600, height: 900, occluded: false });
published.set(Surface { width: 1920, height: 1080, occluded: false });
published.modify(|surface| surface.occluded = true);

// Once per frame, in the consumer. Three publications, one observation: the
// two in the middle are gone, and nothing can tell they happened.
assert_eq!(
    watch.changed_since(&mut seen).as_deref(),
    Some(&Surface { width: 1920, height: 1080, occluded: true }),
);
assert_eq!(watch.changed_since(&mut seen).as_deref(), None);
```

## Why this is the first `std` crate here

Every crate in the simulation and client rings is `no_std`, and several of them
say why in their own front page: a simulation that cannot name a clock, a
filesystem or an environment variable is a simulation with fewer ways to
diverge, and the compiler is the thing holding that line.

This crate is on the other side of it. What it carries state between is
*threads*, and a thread is the platform's to give -- `Mutex`, `Condvar`, and the
parking a `Condvar` is built out of are all `std`. There is no `alloc`-only
version of this type that does anything, so there is no feature to put it
behind, and pretending otherwise would mean an `alloc` build that compiled and
could not block.

The boundary is therefore drawn here rather than one crate further in. Nothing
in the simulation ring depends on this crate, and nothing in it should: a signal
is how the platform ring and the client ring talk to each other while a game is
running, and a tick is handed its inputs rather than reading them.

## A signal is state, and never an event

The two words are doing real work. **State** is a thing that has a current
value: a window size, a device list, a connection status, the set of peers. It
is meaningful to ask what it is right now, and an older answer is worthless the
moment a newer one exists. **An event** is a thing that happened: a key was
pressed, a packet arrived, a player asked to jump. It is meaningful only as an
occurrence, and skipping one is not the same as being slightly behind.

This type is built for the first and would silently destroy the second. It is
not that events are discouraged here; it is that dropping is what it *does*:

```rust
use corvid_signal::channel;

/// A player's intent for one tick, as a game would define it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action { Idle, Jump }

let (published, watch) = channel("actions", Action::Idle);
let mut seen = watch.seen_now();

published.set(Action::Jump);
published.set(Action::Idle);

// The jump never existed as far as this consumer is concerned. Every other
// peer in the session folded it into their state; this one did not, and the
// two states are now different for the rest of the session.
assert_eq!(watch.changed_since(&mut seen).as_deref(), Some(&Action::Idle));
```

So actions, packets and commands do not travel this way. A dropped action is a
desync -- one peer's simulation folded it in and another's did not, and the
digests part company at that tick and stay parted. Those go on ordered paths
that keep everything and say when they could not, which for a session's actions
is `corvid_replay`'s `ActionLog`: it refuses a write it cannot honour instead of
losing one. A tick's own requests take a path of that kind too -- a tick returns
its commands and `corvid_app`'s runtime routes them in order -- so a game with
commands to route has one already and does not reach for a signal.

The awkward cases are the ones that look like state and are not. "The player is
holding the jump button" is state; "the player pressed jump" is an event, and a
press and a release published between two polls of the first are indistinguishable
from nothing having happened. A cue fired at a tick is an event even though it
sits inside an audio frame that is state. When in doubt, ask whether two peers
that each saw a different subset of the publications would end up in the same
place: if yes it is state, and if no it does not belong here.

## What a consumer remembers

A [`Watch`] holds no per-consumer bookkeeping, so a consumer keeps a [`Seen`] --
eight bytes, `Copy`, holding nothing but how far it has read -- and passes it
back on every poll. That is what lets one `Watch` be cloned to four threads that
poll at four different rates without any of them blocking or skipping another.

| Starting point | The first poll reports |
|---|---|
| [`Seen::default()`](Seen) | the value in the cell, once -- for a consumer that needs the state before it can do anything |
| [`Watch::seen_now`] | nothing, until something is published -- for a consumer that has just read the state itself |

| Call | Returns | Waits |
|---|---|---|
| [`Watch::get`] | `Arc<T>`, always | never for a publication; for a [`modify`](Emitter::modify) that is copying |
| [`Watch::changed_since`] | `Some(Arc<T>)` if this consumer is behind, and [`None`] if it is not | the same |
| [`Watch::blocking_wait`] | `Arc<T>`, once this consumer is behind | yes, by design -- parks the thread until something is published |

The middle answer is worth reading exactly. A **publication** never holds a
reader up, and that is the property this crate exists for:
[`set`](Emitter::set) builds its value before taking the lock and drops the one
it replaced after releasing it, so no line of a `T`'s own code runs inside. A
read alongside it is a reference-count bump.

[`modify`](Emitter::modify) is the exception, and it is the only one. It is
copy-on-write through `Arc::make_mut`, which takes its copy *under the lock a
read also takes* -- so a `modify` that has to copy holds every reader up for one
whole `T::clone`. On a 400 000-entry `Vec<String>` whose copy costs 19 ms, a
`get` issued once that copy had begun came back after 23 ms;
`examples/signal_bench.rs` measures it and `tests/lock.rs` pins it with a
payload whose `Clone` is rigged to take 300 ms, so the property is asserted
rather than timed. A `modify` that does not have to copy -- nobody holding the
value -- costs a reader nothing.

Neither read blocks for want of a value: there is always one in the cell.
`changed_since` returns `Some` exactly once per *catch-up*, not once per
publication. Three publications between two polls are one `Some` carrying the
third value.

## Why a read hands back an `Arc<T>` and not a `T`

Because the alternative puts the consumer's work on the publisher's path, and
that is the one thing this type exists not to do.

The cell holds an `Arc<T>`. A publication builds its `Arc` before taking the
lock, swaps a pointer and an integer under it, and drops what it replaced after
releasing it -- so no line of a `T`'s own code runs while the lock is held. A read
bumps a reference count under the lock and does everything else outside it. What
a consumer gets back is a snapshot: it stays the value it was handed however many
publications land afterwards, and a consumer that wants to own and edit one
writes `(*value).clone()` on its own thread and its own time.

Handing back a `T` instead would mean cloning it, and the only place a clone can
be taken from a shared cell consistently is under the lock. That is what this
crate did before, and it is what the middle row below measures:

| | value in the `Mutex` | `Arc` in the `Mutex` |
|---|---|---|
| `Watch::get` on a 400 000-entry `Vec<String>` | 13.4 ms -- a whole copy | 17 ns |
| one publication issued while a consumer is copying | 28.9 ms | 1.4 us |
| 200 publications with nobody watching | 5.7 ms each | 5.7 ms each |
| the same 200 with one consumer copying throughout | 5.8 ms each | 5.8 ms each |

`examples/signal_bench.rs`; the figures move with the host and the first two rows
are four orders of magnitude apart, which does not. Only the right column can be
re-run from this tree -- the left one is what the same example printed against the
design that is gone, so read it as recorded rather than as reproducible. What
the example still measures on demand is the copy itself, and the left column's
first row *is* that copy: a read that has to clone under the lock costs whatever
cloning the value costs, which was 19 ms on the machine this paragraph was last
checked against.

Read the bottom two rows as carefully as the top two, because they are the ones
that say what did *not* change. Sustained throughput is the same either way -- a
publisher that spends 40 ms building a 400 000-entry list is only inside `set`
for a small fraction of its cycle, so it rarely met the consumer in the lock even
before. What moved is the worst case, and the worst case is what a frame-paced
publisher is made of: a platform thread that publishes a window size at 90 Hz has
11 ms to work with, and a 29 ms stall in one of those publications is a dropped
frame that a throughput average will never show.

Most of the 5.7 ms a publication costs in either column is freeing the list it
replaced -- 400 000 deallocations -- which is the publisher's own work and happens
after the lock is released.

### What that costs

Two things, said plainly.

`modify` becomes copy-on-write. `Arc::make_mut` edits the value where it lies
when the cell holds the only reference to it, and clones the whole `T` first when
a consumer is holding the value about to be edited. So a device list that gained
one entry is still one push on a signal nobody is reading right now, and a push
plus a copy of the list on one that somebody is -- at most one copy per
publication however many consumers there are, and never more work than the
clone-edit-`set` it replaces. `tests/lock.rs` counts the copies.

That copy is the one place in the crate where a `T`'s own code runs inside the
lock, so it is the one place a reader waits: a `get` that arrives during it
waits the length of the clone, which the table near the top of this file says
and `tests/lock.rs` asserts. A publisher that cannot afford to hold readers
up for a copy of the value should reach for `set`, which never takes one -- the
trade is that `set` needs a whole new value and `modify` needs only an edit.

And `T: Sync` is required, not merely `T: Send`. That is not a bound to be
papered over: handing an `Arc<T>` to two threads *is* sharing a `T` between them,
which is exactly what `Sync` means. The section below says what that rules out.

### `blocking_wait` is for a thread with nothing else to do

A thread whose entire job is to react to one signal, and which sleeps between
reactions, is what it is for. **No frame-rate or tick-rate path may call it.** A
frame loop that parks in here has handed its pacing to whichever subsystem
publishes next; a tick loop that parks in here has handed the fixed step to a
producer that knows nothing about it. Those two paths own their own clocks and
poll `changed_since` instead. Nothing here can tell which thread it is on, so
this is a rule somebody keeps rather than one the compiler enforces.

There is one more thing a thread parked in there owes. When every [`Emitter`]
for a signal has been dropped, `blocking_wait` parks forever -- the signature
returns a `T` and there is no value to invent, so there is nothing else it could
do. A thread that has to be able to exit needs a way out that does not come
through that call: its own shutdown flag, and a last publication from whoever
sets it, which is exactly the shape `tests/blocking.rs` uses to shut its own
threads down.

## Every publication opens a span

`tracing` is a hard dependency and not a feature, because a handoff between two
threads with only one end instrumented is not worth instrumenting. Publishing
opens a span; observing leaves an event; both carry the signal's label and the
sequence number of the publication, so the two ends of one handoff can be joined
in a trace and the interval between them read off. Nothing here measures that
interval -- the two timestamps are the subscriber's to record.

| Callsite | Level | Fields |
|---|---|---|
| `corvid_signal.set` (span) | `DEBUG` | `signal`, `sequence` |
| `corvid_signal.modify` (span) | `DEBUG` | `signal`, `sequence` |
| `corvid_signal.observed` (event) | `TRACE` | `signal`, `sequence` |

The label is a parameter of [`channel`] rather than something optional, because
a span named after nothing is a span nobody can read: a trace of six subsystems
publishing state has to say which of them published. It names the state rather
than the type -- `"surface"`, `"peers"`, `"audio devices"` -- since two signals
may well carry the same type. `tests/tracing.rs` installs a subscriber and reads
the spans back, which is the only way a claim about a trace is a claim rather
than a hope.

The span name is a constant and the label is a field, rather than the other way
round, because `tracing` builds a callsite's metadata at compile time and a span
name is part of it.

## `Send`, `Sync`, and exactly when

Both handles are `Send + Sync` when `T: Send + Sync`, and neither is when it is
not. Both halves of that bound are load-bearing. `Send` because a value crosses a
thread boundary, which is what the type is for; `Sync` because two consumers can
be holding the same `Arc<T>` and reading it at the same time, and that is
sharing rather than sending.

`Sync` was not required while the cell held the `T` itself, and a `T` that could
cross threads without being shared between them -- anything built on `Cell` --
could travel here. It cannot any more, and that is the price of the section
above. A game with such a value has two ways round it: publish something the
`Cell` was standing in for, or wrap it in the synchronisation the `Cell` was
avoiding.

```rust
use corvid_signal::channel;

fn needs_send_sync<T: Send + Sync>(_: &T) {}

let (published, watch) = channel("plain", 0_u32);
needs_send_sync(&published);
needs_send_sync(&watch);
```

```rust,compile_fail
use std::cell::Cell;
use corvid_signal::channel;

fn needs_send_sync<T: Send + Sync>(_: &T) {}

// `Cell<u32>` is `Send` and is not `Sync`, and neither handle is either. This
// block compiled before a read handed back an `Arc`, and it is here as a
// `compile_fail` rather than deleted because a bound that quietly loosens again
// should show up as a doctest that stopped failing.
let (published, watch) = channel("interior", Cell::new(0_u32));
needs_send_sync(&published);
needs_send_sync(&watch);
```

```rust,compile_fail
use std::rc::Rc;
use corvid_signal::channel;

fn needs_send_sync<T: Send + Sync>(_: &T) {}

// `Rc` is neither, so the handle carrying it is neither: this is the line that
// does not compile, and `Rc<u32>` cannot be sent between threads safely is what
// it says.
let (published, _watch) = channel("shared count", Rc::new(0_u32));
needs_send_sync(&published);
```

```rust,compile_fail
use std::rc::Rc;
use corvid_signal::channel;

fn needs_send_sync<T: Send + Sync>(_: &T) {}

// The same for the observing end, which is a separate type and gets the
// property for the same reason rather than by inheriting it.
let (_published, watch) = channel("shared count", Rc::new(0_u32));
needs_send_sync(&watch);
```

## What holds each promise

The distinction worth being exact about is between what the compiler refuses to
build and what somebody has to keep true.

| | Held by |
|---|---|
| Both handles are `Send + Sync` exactly when `T: Send + Sync` | the compiler, and the four blocks above |
| A consumer never observes a value assembled out of two publications | one `Mutex` around one pointer, swapped whole; a reader takes the pointer and never the parts |
| A publication never waits for a consumer | nothing a consumer does runs under the lock -- no `Clone`, no `Drop`, no allocation -- and nothing on the publishing path waits on the condition variable |
| One publication wakes every parked consumer, not one of them | `notify_all`, and `tests/blocking.rs` parks eight threads on one publication for each of the two publishing calls |
| A signal never grows | the cell holds one `Arc<T>`, and a publication drops the one it replaced |
| A value handed to a consumer does not change underneath it | the `Arc` it was handed, and `modify`'s copy-on-write step |
| Only state travels here, never actions or packets or commands | **the caller.** Nothing checks, and the failure is a desync |
| No frame-rate or tick-rate path calls [`blocking_wait`](Watch::blocking_wait) | **the caller.** Nothing here can tell which thread it is on |
| A [`Seen`] is polled against the [`Watch`] it was polled with before | **the caller.** A `Seen` carries no channel identity, so a mismatch reports a change that never happened or misses one that did |
| A [`modify`](Emitter::modify) closure does not touch this signal | **the caller.** It runs under the lock, and `std`'s `Mutex` is not reentrant, so it deadlocks |
| A thread parked in [`blocking_wait`](Watch::blocking_wait) can be woken to exit | **the caller**, as above |

Two smaller things the implementation does rather than promises. The value a
publication replaced is dropped *after* the lock is released, so a `T` whose
`Drop` publishes to the same signal works instead of deadlocking -- that is one
re-entrant path made to work, not a general rule, and the `modify` closure and
the copy it may take first both still run under the lock -- as, in one race that
`modify` documents, can the drop of the value that copy was taken from. And a
poisoned mutex is ignored rather than propagated, because the workspace denies
`unwrap_used` and the only other answer is a signal that stops carrying a window
size because something unrelated panicked once. What a panicking `modify` leaves
in the cell is documented on [`modify`](Emitter::modify), is the caller's to
think about, and is checked rather than described: `tests/channel.rs` panics
halfway through one and reads back what survived.

## Nothing here is written down

The workspace's rule is that every type which is serialized owes two golden
tables -- the bytes, and a self-describing encoding for what bytes cannot see.
This crate owes neither, because it serializes nothing: a signal is one process
talking to itself, and no value ever reaches a disk, a socket, or another build.
There is no `serde` in the dependency list and nothing to freeze.

That is not an exemption granted to this crate; it is what having no wire format
looks like. The day a value that travels through a signal is written down, it is
written down by the crate that *defines* that value, and that crate owes both
tables for it.

## Tests

```sh
cargo test -p corvid_signal
```

| File | Covers |
|---|---|
| `tests/channel.rs` | Latest-value semantics: that three publications between two polls are one observation of the third; that a poll reports a change exactly once and two watchers keep their own place; that a value handed out does not change underneath its reader; that both handles name the signal and print nothing of the value, formatted from inside a `modify` closure that is holding the lock; and what a panicking `modify` leaves in the cell, does not publish, and does not break |
| `tests/blocking.rs` | That a consumer that never polls and eight consumers parked in `blocking_wait` neither hold up nor grow a publisher; that one publication -- by `set` and by `modify` -- wakes all eight parked consumers rather than one; and that `blocking_wait` wakes, and returns without parking when it is already behind |
| `tests/lock.rs` | What runs outside the lock: that the value a publication replaced is dropped there; that a consumer half a second into copying the value holds no publisher up; and that `modify` copies the value exactly when a consumer is holding it and never otherwise |
| `tests/threads.rs` | eight publishing threads and eight observing threads on one signal, half of them parked and half polling, checking every observation against a seal over its own fields, against the range of values anybody published, and against a ticket that climbs for every author -- plus the check that the seal really does reject a reading assembled out of two publications |
| `tests/tracing.rs` | a recording subscriber, reading back that a publication opens a span carrying the signal's label, level and sequence number, that an observation leaves the matching event at the level the table above gives it, and that a poll which saw nothing leaves nothing |
| `examples/signal_bench.rs` | the four figures in the table above, on whatever host runs it |
| doctests | every `rust` block on this page, including the three that must fail to compile |

The blocking tests are the ones worth a note. A test about a thread *not*
waiting fails by hanging, which is a red build with no message or a CI job
killed an hour later, so the call under test runs on a thread of its own and the
assertion is that it came back inside ten seconds. Ten is not a performance
bound; nothing here asserts that anything took less than a millisecond.

Two things the stress test does not show, said plainly because the test's name
sounds like it does. A `Mutex` cannot tear, so the seal is a guard on whatever
replaces the `Mutex` rather than a failing test today -- it earns its place the
day somebody reaches for two atomics because the lock showed up in a profile.
And that no consumer *saw* a torn value across those sixteen thousand
publications is a statement about the interleavings that run happened to
produce, not about every interleaving there is.
