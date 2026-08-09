# `corvid_control`

What a player is doing, where they are looking, and what their pad should feel.

One of the four types an `App` is made of, and the one that is a *player*. A
game may have several: a keyboard and a pad is one, a scripted opponent is
another, a replay is a third — each a different type implementing `Controller`
rather than a flag inside one.

```rust,ignore
impl Controller<Table> for Hands {
    type Config = Sensitivity;
    const SETS: &'static [SetDescriptor] = action::SETS;

    fn new(config: Sensitivity) -> Self { /* … */ }
    fn configure(&mut self, config: Sensitivity) { self.sensitivity = config; }

    fn update(&mut self, updating: Updating<'_, Table>) { /* the camera moves here */ }

    fn look(&self) -> Camera { self.eye.camera() }
    fn action(&self, acting: Acting<'_, Table>) -> Move { /* … */ }
}
```

## `update` writes, everything else reads

| Function | Runs | Writes |
|---|---|---|
| `update` | once per **displayed frame** | `&mut self` |
| `look` | once per displayed frame, after `update` | nothing |
| `action` | once per **tick** | nothing |
| `rumble` | once per tick | nothing |

A fifteen-hertz simulation on a hundred-and-forty-four-hertz display calls the
bottom two fifteen times a second and the top two nine or ten times as often, at
a rate nobody chose and nothing records.

**That split is the point.** One method mutates and three read, so a `look` that
moved the camera it was reporting stops being expressible. Handing all four a
shared `&View` instead would only owe that discipline rather than enforce it: a
type bounded by `Default` may hold a `Cell`, so every one of the reading
functions could write through it, and "the view moves in exactly one place"
would be a paragraph instead of a signature.

## Nothing here compiles a graphics stack

A `Controller` names a `Camera`, an `Input` and a `State`. None of those knows a
device exists, and that is deliberate: putting the *camera* on the far side of a
`wgpu` dependency from the code that moves it would make a game with no renderer
at all compile a device in order to say where its eye was.

## What goes on the wire is the action, not what it read

`action` may read the camera, the cursor and the clock, because none of them
leaves this machine. A ray cast from this machine's pointer, resolved against
this machine's camera, arrives at every other peer as `Action::Aim { at }` — a
value in the game's own vocabulary, which every peer folds into the state the
same way.

So the rule is about what the *action* denotes rather than about what `action`
may look at. An action that names a target is fine. An action that names a
screen pixel, a viewport-relative offset or a number of display frames is not,
and nothing here can tell those apart.

## Rumble is here, not in `Command`

A haptic reaches a device exactly one peer has. Routing it through a
deterministic tick would put it on the wire and behind a network round trip to
get there. That is the same argument the camera and the pointer are client-local
for, and it has the same answer.

It is asked once per **tick** rather than once per frame, so an effect fires
exactly once for the tick that earned it and there is no retrigger to
deduplicate.

## `()` is nobody at the controls

The default for an `App`'s controller. It declares no actions, wants no devices,
submits the idle action forever and looks at the origin — which is exactly what
a dropped player does, so a seat driven by it is a seat nobody is sitting in and
the simulation already knows what to do with one.

`REAL = false` is what tells the runtime to open no window and read no keyboard.
It is about the *platform*, not about whether the controller runs: a bot has
`REAL = false` and is still asked for an action every tick.
