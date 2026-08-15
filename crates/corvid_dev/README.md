# `corvid_dev`

The console, the tunables, the time slider, the inspector and the overlay.

Nothing here draws. A `Console`'s entries, a `Rows`, an `Overlay` and a
`HelpLine` are descriptions a surface renders; the surface is `corvid_ui`'s, and
the `ui` feature is where it attaches.

## The registry is typed, so help and completion are generated

A command is the closure its signature already describes. `Option<u32>`
implements `Argument`, which carries a `parse`, a `const TYPE` and a
`const REQUIRED`; a closure's parameters implement `Arguments`, which yields a
`&'static [Parameter]`. Help is printed from that slice and completion is
offered from it, so a command whose signature changes has its help change with
it and nothing is written twice.

```rust
use corvid_dev::{Console, Invalid, Reply};

let mut console = Console::new();

console
    .register("wave.skip", "advance to the next wave", |to: Option<u32>| {
        Reply::said(format!("skipping to {to:?}"))
    })
    .named(&["to"]);

console.register("wave.stop", "stop the wave", || Reply::Done);

// The usage line comes off the type. The brackets are `REQUIRED = false`.
assert_eq!(console.help(Some("wave.skip"))[0].usage, "wave.skip [to: u32]");

// So does the completion.
let under = console.complete("wave.");
assert_eq!(under.len(), 2);
assert_eq!(under[0].text, "wave.skip");
assert_eq!(under[0].help, "advance to the next wave");

// An unknown command answers and suggests nothing: a console that runs the
// wrong command is worse than one that runs none.
assert_eq!(
    console.run("wave.skop"),
    Reply::Refused(Invalid::Unknown { path: "wave.skop".to_owned() }),
);
```

Only types implementing `Argument` may be parameters. The primitives,
`String`, `Tick`, `I16F16`, and `Option<T>` and `Vec<T>` over any of them are
here; a command wanting a bespoke value implements the trait for it in four
lines, which is the friction that keeps a console command from becoming an API.

Rust does not expose a closure's parameter names, so parameters arrive numbered
and `Registered::named` is how a command says what its help should read.

## A tunable proposes and never writes

`Rules` is hashed, so changing one in a session is a change every peer must
accept. `Tunable`'s accessors are private and `Tuning::propose` takes the rules
by shared reference: there is no method here that writes into a live `Rules`,
which makes the rule unwriteable-wrongly rather than documented-carefully.

```rust
use corvid_dev::{Invalid, Tunable, Tuning};
use corvid_hash::digest;
use corvid_time::Tick;
use corvid_fixed::I16F16;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Rules {
    damage: I16F16,
}

let mut tuning = Tuning::new();
tuning.register(Tunable::new(
    "tower.arc.damage",
    I16F16::ZERO..=I16F16::from_f64(500.0),
    |rules: &Rules| rules.damage,
    |rules: &mut Rules, to| rules.damage = to,
));

let live = Rules { damage: I16F16::ONE };
let before = digest(&live);

let proposal = tuning.propose(&live, "tower.arc.damage", I16F16::from_f64(12.5), Tick(40))?;

// The live rules are untouched, and the proposal reaches the hash.
assert_eq!(digest(&live), before);
assert_ne!(digest(&proposal.rules), before);

// Outside the range it refuses, and names the range.
assert!(matches!(
    tuning.propose(&live, "tower.arc.damage", I16F16::from_f64(900.0), Tick(40)),
    Err(Invalid::OutOfRange { low, high, .. }) if high == I16F16::from_f64(500.0) && low == I16F16::ZERO,
));
# Ok::<(), Invalid>(())
```

## The rest

`Slider` is `Session::seek` with a range on it -- there is no second replay here
to keep in step with the one every save, load and rollback already goes through.
`Inspect` is a game naming its own rows, because reflection over a `State` would
be a second serialization format that can disagree with the first. `Overlay`
carries the digest, the tick, the rollback depth and the frame count, every one
of them a public field a runtime fills in. `dump_frame` prints what a whole
frame digests to and then what each of its parts does -- a frame owns its two
states, its level and its rules now rather than borrowing them, so one number
covers the lot, and the four beneath it are what says which part moved. That
same ownership is why `dump_frame` has a doctest at all: a frame is six values
a test writes down, with no loop behind it.

`dump_audio` does the same for an `AudioFrame`.

Nothing here names the runtime. The runtime is what reads these and puts them
on a screen, so the dependency runs from there to here -- which is what leaves a
facade free to reach this crate through `corvid_app`.
