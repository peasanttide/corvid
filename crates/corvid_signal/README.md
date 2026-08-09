# `corvid_signal`

Latest-value cells for Corvid: how one subsystem tells another what the world
looks like now, across a thread boundary, without either of them waiting for the
other.

A signal holds exactly one value. Publishing replaces it, observing hands back a
shared handle on whatever is there, and everything published between two
observations is dropped and cannot be recovered.

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

**A signal carries state, and never an event.** State is a thing that has a
current value -- a window size, a device list, a connection status -- where an
older answer is worthless the moment a newer one exists. An event is a thing
that happened, and skipping one is not the same as being slightly behind.
Dropping is what this type *does*, so an action, a packet or a command sent this
way is a desync waiting to happen: one peer folded it into its state and another
did not. When in doubt, ask whether two peers that each saw a different subset
of the publications would end up in the same place.

An [`Emitter`] publishes and a [`Watch`] observes, both cheap to clone and both
`Send + Sync` exactly when `T` is. A `Watch` holds no per-consumer bookkeeping;
a consumer keeps a [`Seen`] -- eight bytes, `Copy`, holding nothing but how far
it has read -- and passes it back on every poll, which is what lets one signal
serve four threads polling at four different rates. [`Watch::get`] always
answers, [`Watch::changed_since`] answers once per catch-up rather than once per
publication, and [`Watch::blocking_wait`] parks until there is something new.

A read hands back an `Arc<T>` rather than a `T`, and that is the design. The
cell holds an `Arc`, so a publication builds its value before taking the lock,
swaps a pointer under it, and drops what it replaced after releasing it -- no
line of a `T`'s own code runs inside the lock, and a read alongside it is a
reference-count bump. What a consumer gets is a snapshot that stays what it was
handed however many publications land afterwards, and one that wants to own and
edit a value clones it on its own thread and its own time.

[`Emitter::modify`] is the one exception. It is copy-on-write through
`Arc::make_mut`, which takes its copy under the lock a read also takes, so a
`modify` that has to copy holds every reader up for one whole `T::clone`. A
publisher that cannot afford that reaches for [`Emitter::set`], which never
copies; the trade is that `set` needs a whole new value where `modify` needs
only an edit.

`tracing` is a hard dependency rather than a feature, because a handoff between
two threads with only one end instrumented is not worth instrumenting.
Publishing opens a span and observing leaves an event, both carrying the
signal's label and the publication's sequence number, so the two ends of one
handoff join up in a trace. The label is a parameter of [`channel`] rather than
something optional, since a trace of six subsystems publishing state has to say
which of them published.

Two rules the compiler cannot keep. Only state travels here, as above. And no
frame-rate or tick-rate path calls [`Watch::blocking_wait`]: a loop that parks
in there has handed its pacing to whichever subsystem publishes next, and a
thread parked there when the last [`Emitter`] is dropped waits forever.

## Scope

State, and never an event. A signal holds one value, so nothing here queues,
buffers or replays. A subsystem that needs every item wants a channel instead.

One producer type and one consumer type over a `Mutex` and a `Condvar`, which is
what a thread boundary costs. There is no async form and no executor
integration: a consumer either polls or blocks. Carrying state between processes
or across a network is somebody else's problem, since this is threads in one
address space.
