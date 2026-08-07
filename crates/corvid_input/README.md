# `corvid_input`

One frame of input for a [Corvid](https://github.com/peasanttide/corvid) game,
as data. A game declares the actions it can be asked to perform; hardware is
bound to those declarations somewhere else, and the game never sees a key code.
This is Steam Input's action-set model taken literally, because it is the one
that survives six device kinds and a rebinding screen.

This crate is the `no_std` half of that: the declaration, the identifiers it
hands out, and [`Input`], which is one frame's worth of digital, analog and pose
values. It opens no windows and reads no files. The half that talks to devices sits
behind the `platform` feature: a device-neutral control vocabulary, a binding
table, and the accumulator that works out the edges. Hot-plug, glyph lookup and
rumble are not there.

What consumes an [`Input`] is `corvid_present`, whose `intend` turns a snapshot
into one player's action for one tick. This crate stays `no_std` and device-free
so that it sits in the client ring without dragging an operating system in behind
it.

```rust
/// The one place this game's actions are named.
pub mod action {
    corvid_input::action_sets! {
        pub set Build {
            digital PLACE, CANCEL;
            analog LOOK, MOVE;
            pose POINTER;
        }
        pub set Console {
            digital SUBMIT, DISMISS;
            analog SCROLL;
        }
    }
}

fn main() {
    use corvid_fixed::Signed16;
    use corvid_input::{Analog, Digital, Input};

    // The snapshot is sized once from the declaration. Filling it is the
    // device layer's job; here it is a test doing it by hand.
    let mut input = Input::new(action::SETS);
    input.set_digital(action::PLACE, Digital::HELD);
    input.set_analog(
        action::LOOK,
        Analog::new(Signed16::from_bits(30_000), Signed16::ZERO),
    );

    assert_eq!(input.active_set(), action::Build::ID);
    assert!(input.digital(action::PLACE).held);

    // The console opens, and it does not have to know what the game bound to
    // that button. It activates its own set, and the game's actions stop
    // answering — including the one the player is still holding down.
    input.activate(action::Console::ID);

    assert_eq!(input.digital(action::PLACE), Digital::RELEASED);
    assert_eq!(input.analog(action::LOOK), Analog::ZERO);

    // And the console closes. The button was never let go of, so it reads as
    // held again: silencing an action is a view of the device, not an edit of
    // it.
    input.activate(action::Build::ID);
    assert!(input.digital(action::PLACE).held);
}
```

## One set answers, and the rest read as released

Exactly one set is active. A query about an action outside it returns
[`Digital::RELEASED`], [`Analog::ZERO`] or `None` — not the value the device is
producing, and not the value the action last had while its set was active. That
second half is the one that matters: an overlay that only silenced values
arriving after the switch would let a button the player was already holding fire
one more time, on the frame the console opened, in whatever the game was doing
underneath.

The values are kept rather than cleared, and the query is what masks them. That
is what makes the two sets independent in both directions: neither has to be
told about the other, and handing control back reads the device as it is now
rather than as whatever the last frame before the overlay happened to see.

The layering is one active set rather than a stack. A console over a game is
two sets and a call to `activate`, and whoever is doing the overlaying keeps the
set to go back to. A stack, and the fall-through where an action absent from the
top set is looked for in the one below, is not here — a rebinding screen is
what would give it a shape worth committing to.

The pointer is deliberately outside all of this. It is not an action and belongs
to no set, so activating another set does not silence it: an overlay wants the
cursor as much as the game did.

## Declaration order is a wire format

`action_sets!` hands out identifiers densely from declaration order and from
nothing else — sets numbered from zero as they are written, and each kind of
action numbered from zero in its own space, so a set's actions of a kind are a
contiguous run and asking whether an identifier belongs to a set is two
comparisons.

```rust
pub mod action {
    corvid_input::action_sets! {
        pub set Menu {
            digital NAVIGATE_UP, NAVIGATE_DOWN, ACTIVATE, BACK;
        }
        pub set Build {
            digital PLACE, CANCEL;
            analog LOOK, MOVE;
        }
    }
}

/// The same two sets, written down in the other order.
pub mod swapped {
    corvid_input::action_sets! {
        pub set Build {
            digital PLACE, CANCEL;
            analog LOOK, MOVE;
        }
        pub set Menu {
            digital NAVIGATE_UP, NAVIGATE_DOWN, ACTIVATE, BACK;
        }
    }
}

fn main() {
    use corvid_input::{AnalogId, DigitalId, SetId};

    assert_eq!(action::Build::ID, SetId(1));

    // `Build`'s digital actions carry on from `Menu`'s four…
    assert_eq!(action::PLACE, DigitalId(4));

    // …and its analog actions start at zero, because `Menu` declared none and
    // the two kinds are numbered apart.
    assert_eq!(action::LOOK, AnalogId(0));

    // The table both came out of, which the binding layer and the rebinding
    // screen will read.
    assert_eq!(action::SETS[1].name(), "Build");
    assert!(action::SETS[1].digital().contains(action::PLACE.0));

    // Swapping the two moves every digital action in both sets, and not all of
    // them by the same amount — `Build` declares two and `Menu` four…
    assert_eq!(swapped::PLACE, DigitalId(0));
    assert_eq!(swapped::NAVIGATE_UP, DigitalId(2));

    // …and moves no analog action at all, because `Menu` declares none and so
    // `Build`'s analog run started at zero under either order. A binding on
    // `LOOK` came through a reorder that broke every binding next to it.
    assert_eq!(swapped::LOOK, action::LOOK);
}
```

A binding file that recorded this player's `X` button as `DigitalId(4)` would
outlive the build that wrote it. **Reordering a declaration, or inserting an
action anywhere but at the end of its set, re-points the run of identifiers from
the edit onwards** — such a file would still parse, still name a real action, and
name the wrong one.

It is a run and not the whole file, and the difference is the thing worth being
exact about. Identifiers ahead of the edit keep their numbers, and so does every
identifier of a kind the edit did not disturb — which is what the `swapped`
module above is doing there: it moves every digital action of both sets and
leaves `LOOK` exactly where it was, because `Menu` declares no analog actions
and so `Build`'s analog run never moved. So a migration that spot-checks one
binding, finds it pointing where it always did, and concludes the file survived
has learnt nothing. "Every binding is invalid" is the wrong warning to give even
though it sounds like the careful one: it is false in a direction that makes the
surviving bindings look like evidence.

**So a file does not record the number.** `action_sets!` generates the *name* of
every action beside its identifier — the spelling the programmer declared it
under, which is the one thing about an action that does not move — and
`platform::Table` is a binding table written down in those names, which is what
`corvid_app` reads and writes as JSON:

```json
{ "buttons": [{ "control": "W", "action": "PLACE" }] }
```

A file like that survives every reorder above. What it does not survive is an
action being *renamed* or removed, and that is the trade taken deliberately:
renaming is a thing somebody did on purpose and can be told about, where
reordering is a thing that happens while you are looking at something else. A
name this build does not declare is refused with the word in it rather than
resolved to whatever now sits at that number.

Numbers are still what a snapshot is indexed by, and the paragraph above is
still true of anything that records one. Nothing here can detect that break,
because the build that wrote it is not present to be compared against. What is here instead is `tests/golden.rs`, which
freezes the numbering for a fixed declaration as literals, so that the change is
a red test at home before it is a wrong button in front of a player. Adding a
set at the end, or an action at the end of the last set that declares that kind,
moves nothing.

The identifiers are the reason the numbering is dense rather than hashed from
the action's name. A name that hashed would survive reordering and would break
on a rename instead, which is the opposite of the trade wanted here: an action is renamed far more often than a
set is reordered, and a dense number is also what makes membership a range check
and a binding table an array.

## What it generates

A `const SETS: &[SetDescriptor]` holding one descriptor per set in declaration
order; a unit struct per set, named as the set was declared, carrying that set's
`ID` and `NAME`; and one constant per action, named as the action was declared,
of the identifier type its kind calls for.

| Declaration | What a game writes | Type |
|---|---|---|
| `pub set Build { … }` | `action::Build::ID` | [`SetId`] |
| `digital PLACE;` | `action::PLACE` | [`DigitalId`] |
| `analog LOOK;` | `action::LOOK` | [`AnalogId`] |
| `pose POINTER;` | `action::POINTER` | [`PoseId`] |
| the whole invocation | `action::SETS` | <code>&[[SetDescriptor]]</code> |

Action constants land in the module the macro was invoked from rather than
inside their set's struct, which is why the example above wraps the invocation
in `mod action` and writes `action::LOOK`. Two actions sharing a name is
therefore a duplicate-definition error, which is the same rule Steam Input's
manifest works under. `SETS` is generated under that name every time, so one
invocation per module.

The numbering is computed by [`layout`], a `const fn` this crate exports, rather
than by the macro counting as it goes. That is deliberate: the rule is then one
function with a doctest and a golden rather than a thing the expansion happens
to do, and a table built by hand goes through the same door.

## A stick and a mouse are not the same number

A stick reports a **deflection**: how far it is pushed, which is a rate, so what
reads it multiplies by the frame's `dt`. A mouse reports the **motion that
already happened**: the pixels a frame accumulated, which are already
proportional to how long that frame lasted, so multiplying by `dt` again turns a
camera by the square of the frame time — smooth at a steady frame rate, and
visible as shake the moment the rate wobbles.

So they are two accessors and the names say which is which. `Input::analog` is
the deflection and `Input::delta` is the displacement, and a binding declares
which one it answers on with `Reading`. An action bound to a stick reads zero
from `delta`, and one bound to a mouse or a wheel reads zero from `analog`:

```rust
use core::num::NonZeroU32;
use corvid_input::platform::{Axis, Bindings, Devices, Reading};
use corvid_input::{Analog, Input};

mod action {
    corvid_input::action_sets! {
        pub set Playing {
            analog LOOK;
        }
    }
}

# fn main() {
let Some(span) = NonZeroU32::new(100) else { return };
let mouse = Bindings::new().axis(Axis::MouseMotion, action::LOOK, span, Reading::Displacement);

let mut devices = Devices::new();
let mut input = Input::new(action::SETS);
devices.moved(Axis::MouseMotion, 100, 0);
devices.snapshot(&mouse, &mut input);

// A full span of motion is a full sweep of the action…
assert_ne!(input.delta(action::LOOK), Analog::ZERO);
// …and the deflection accessor stays where it was, because nothing here is a
// deflection. Reading the wrong one is a value that does not move, which is a
// mistake that finds itself.
assert_eq!(input.analog(action::LOOK), Analog::ZERO);
# }
```


### Two buttons are a stick

`Bindings::pair` binds `S` and `W` to one *component* of an analog action, which
is how a player without a pad pushes a stick forwards and backwards. It composes
in the binding table rather than in the game: an action bound this way reads as
a deflection whether the player used keys or a stick, so a game that supports
both writes one code path and no `if`.

A pair has **no `reading` field**, and that is the point. A held button means
"keep going", which is a rate; there is no quantity in a button for a
displacement to be made of, so the type does not offer a way to say otherwise.
Both halves held is exactly centred, so leaning on left and right together stands
still rather than creeping.

### Gamepads

`Button::Pad` names a pad button by **where it sits** — `South` is the bottom
face button on every pad, whatever letter is printed on it — for the same reason
`Key` names a key by position. `Axis::LeftStick`, `RightStick` and `Triggers` are
the analog half, and they are *levels*: a stick held over is still held over on a
frame the platform said nothing about, which is what `Reading::Deflection` and
`Devices::deflected` were built for and what nothing in this workspace used until
there was a pad to read.

Nothing here reads a device. `corvid_window`'s `gamepad` feature is the adapter,
and it is the only file in the workspace that names a pad backend.

`Bindings::placeholder` binds the mouse and the wheel as `Reading::Displacement`,
because that is what a desktop reports. Nothing here reads a device that produces
a deflection, so `Devices::deflected` exists and is called by nothing in this
workspace but the tests, which is the honest state of it rather than a claim
that sticks are supported.

## No floating point

An analog axis is a [`Signed16`] and a pose is a `GlobalFineTransform`, both from
`corvid_fixed` by way of `corvid_transform`. A stick position that reached a
game as `f32` would be a different number on a machine that rounded differently,
and this crate is the last thing between a device and a tick that has to run
identically on every peer.

[`Digital`] carries the level and both edges — `held`, and `pressed` and
`released` for the frames the action changed on — so a game that wants "this
frame, once" does not have to remember last frame's answer. No combination is
rejected, and the one that looks wrong is not: a tap that starts and finishes
inside a frame arrives as `pressed` and `released` with `held` false, which is
the honest report of what happened and the event a game must not miss.
Producing the edges is the job of whatever fills the snapshot; this crate only
carries them, and does not check that a sequence of frames is consistent.

## From an axis to a quantity

An axis and a position are both integers and they are not the same kind of
integer. An axis is `SNORM` — `bits / 32767`, so the ends are exactly ±1 — and
everything a simulation measures in is scaled by a power of two, `I16F16` at
1/65536 and `I24F8` at 1/256. Crossing between them is a multiply and a divide
by an odd number, which is why it is here rather than at every call site: the
alternative was a shift that every game would write the same way and that would
never quite reach the top of its range.

```rust
use corvid_fixed::{I16F16, Signed16};
use corvid_input::{Analog, scale};
use corvid_vector::FinePoint;

// How far a fully pushed stick moves something in one tick.
const SPEED: I16F16 = I16F16::from_f64(0.25);

let stick = Analog::new(Signed16::MAX, Signed16::ZERO);

// One axis at a time…
assert_eq!(scale(stick.x, SPEED), SPEED);
assert_eq!(scale(-stick.x, SPEED), -SPEED);

// …or both, in the +X right, +Y forward plane.
assert_eq!(
    stick.on_the_ground(SPEED),
    FinePoint::new(SPEED, I16F16::ZERO, I16F16::ZERO),
);
```

The ends are exact and the middle is rounded to nearest, symmetrically about
zero: `scale(-axis, full)` is `-scale(axis, full)` for every axis and every
scale. `tests/scale.rs` is where that is measured rather than asserted — it
walks the whole range against an independent computation in `i128`, and it is
fitted against the three cheaper crossings, each of which it catches: a `>> 15`,
which is off at both ends and everywhere else; a truncating divide, which is
symmetric but not nearest; and a floor, which is nearest but leans one way and
so makes a push left shorter than the same push right.

There is deliberately no conversion that picks the scale for you. A stick means
metres per tick in one game and degrees per tick in another, and the number that
says which belongs in `Rules`, where every peer agrees on it.

## The features

`serde` writes the identifiers and the two value types down — a binding file and
an input recording are both somebody else's format, and this supplies the parts
rather than the file. `std` adds no API and exists so a downstream can forward
it uniformly. `platform` adds the half that faces devices, and adds no
dependency: this crate is `no_std` under every feature it has.

Everything here implements `Hash`, and nothing here is hashed by the simulation.
An [`Input`] is client-local and never crosses into a tick: what crosses is the
action a game derives from it, and that is the game's own type, hashed by
`corvid_behavior`. The derive is for a golden, a dev overlay and a test that
wants one number for a whole snapshot — never for a digest two peers compare.

[`Signed16`]: corvid_fixed::Signed16
