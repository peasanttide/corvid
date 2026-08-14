# `corvid_xr`

Headsets, hands, and both scales, for a [Corvid](https://github.com/peasanttide/corvid)
game. A swarm player holds the planet at arm's length and spins it with a grab;
a defender stands on the surface at human scale and points to build. Both run in
CI on a machine with no headset, because the stand-in is a scripted headset and
the scripted headset is public API.

```rust
use corvid_xr::{Confidence, Headset, PoseTrack, ScriptedHeadset, Space};

// No runtime, no device, no window. Ninety frames of a defender session.
let mut headset = ScriptedHeadset::new(PoseTrack::surface(90));

let mut drawn = 0;
loop {
    let state = headset.poll();
    if state.is_over() {
        break;
    }
    if state.is_drawing() {
        let head = headset.head(Space::Stage);
        // A pose arrives with how much of it to believe, never a bare transform.
        assert_eq!(head.confidence, Confidence::Tracked);
        drawn += 1;
    }
}
// A track records a whole session: two frames open it and three close it, and
// the rest are the ones a game draws.
assert_eq!(drawn, 87);
```

## What is here

| | |
|---|---|
| [`Headset`] | the one trait a game holds: poll, head, views, hands, rumble, passthrough |
| [`ScriptedHeadset`], [`PoseTrack`] | a recording, played at a fixed rate -- the stand-in |
| [`Tracked`], [`Confidence`] | what the runtime reports, and how much of it to believe |
| [`Anchor`], [`Scale`] | where the stage is in the world, and how big a stage metre is |
| [`Views`], [`EyeView`], [`Eye`](corvid_camera::Eye) | the two eyes, and the matrices they want |
| [`Hand`], [`Haptic`], [`Passthrough`] | the rest of the vocabulary |
| `runtime` | `OpenXR` and the Vulkan swapchain, behind the `openxr` feature |

## A pose is a reading, not a transform

Every value a runtime hands back is a [`Tracked<T>`](Tracked): a value, a
[`Confidence`], and when it was true.

```rust
use core::time::Duration;
use corvid_xr::{Confidence, Pose, Tracked};

let behind_the_back = Tracked::inferred(Pose::IDENTITY, Duration::from_millis(33));
assert_eq!(behind_the_back.believed(), Some(Pose::IDENTITY));
assert!(!behind_the_back.is_tracked());

let dropped_on_a_table = Tracked::lost(Pose::IDENTITY, Duration::from_millis(34));
assert_eq!(dropped_on_a_table.believed(), None);
// The last-known value is still there. Fading a hand out is a game's choice;
// teleporting one to the origin is not.
assert_eq!(dropped_on_a_table.value, Pose::IDENTITY);
```

Returning an `Option` would make every call site choose between a jump to the
origin and its own memory of where the hand was. A game that ignores
`confidence` gets a hand frozen where tracking failed, which is the better of
the two defaults: a frozen hand reads as a tracking glitch, and a hand at the
origin reads as a bug in the game.

## One `Anchor`, both scales

```rust
use corvid_xr::{Anchor, Pose};
use corvid_fixed::I16F16;
use corvid_rotation::FineRotation;
use corvid_vector::{GlobalFinePoint, globalfinepoint};

// A defender, standing on a cell. One stage metre is one world metre.
let standing = Anchor::standing(globalfinepoint(0, 0, 2_856), FineRotation::IDENTITY);
assert_eq!(standing.metres, I16F16::ONE);

// A swarm player, holding the same 5 712 m planet as a model a metre across.
let held = Anchor::holding(
    GlobalFinePoint::ZERO,
    I16F16::from_f64(5_712.0),
    I16F16::ONE,
    Pose::IDENTITY,
);
assert_eq!(held.metres, I16F16::from_f64(5_712.0));
```

A stage millimetre at table scale is 5.712 m of world, so **a swarm player
cannot point at a single cell by hand** -- the cell is about 17 um on their
planet. Pointing at table scale is a ray cast from the controller against the
planet in world space, snapped to the nearest cell. The precision does not come
from the hand; it comes from the raycast.

Diving between the two is a camera transition rather than a simulation event, so
it costs nothing, needs no agreement, and cannot desync. That is why an `Anchor`
is the client's own and never appears in an action.

## Why the head is the fine tier

A head pose is a `GlobalFinePoint` (`I48F16`) where a world-space shape is a
`GlobalPoint` (`I24F8`). A shape is an object, and `I24F8`'s 3.9 mm is finer
than anything a cursor can pick. A head pose is where the player's eyes are, and
3.9 mm of jitter at the eye is a visible shimmer on every frame. `I48F16`'s
15.26 um at ten thousand kilometres is what "does not jitter" means, and the
conversion at the boundary is a widening rather than a narrowing, so it is free
and total.

```rust
use corvid_xr::{Anchor, Pose};
use corvid_fixed::I48F16;
use corvid_rotation::FineRotation;
use corvid_vector::{GlobalFinePoint, globalfinepoint};

let far = globalfinepoint(10_000_000, 0, 0);
let anchor = Anchor::standing(far, FineRotation::IDENTITY);

// Fifteen micrometres, ten thousand kilometres out, and the pose is different.
let step = GlobalFinePoint::new(I48F16::DELTA, I48F16::ZERO, I48F16::ZERO);
let here = anchor.to_world(Pose::IDENTITY);
let nudged = anchor.to_world(Pose::new(step, FineRotation::IDENTITY));
assert_ne!(here, nudged);
```

## The one relaxed lint in the workspace

Every crate in this workspace sets `unsafe_code = "forbid"`. This one sets it to
`deny`, in its own manifest, and it is the only one that may. `forbid` cannot be
lifted by an `allow` anywhere in the crate -- that is the difference between the
two words, and it is what makes `forbid` the right default and the wrong choice
for a crate whose job is handing `OpenXR`'s Vulkan handles to `wgpu-hal`. There is
no safe API on either side of that seam.

`src/runtime/vulkan.rs` is the whole seam. It carries a file-level allow with a
reason, and every block in it carries a comment naming the invariant it relies
on and where that invariant is established. `tests/unsafe.rs` enforces both
halves of the claim mechanically:

- the word appears in no other file of this crate, and
- no other `crates/*/Cargo.toml` in the workspace relaxes the lint.

The second test is the one that matters. It is what replaces `forbid`'s
guarantee for this crate, and it is a weaker guarantee: it catches the word, not
the intent.

## The real runtime is optional

The `openxr` feature is **off by default**. With it off, the whole vocabulary
above -- poses, spaces, the anchor arithmetic, the eye matrices, the scripted
headset -- compiles and tests on a machine with no graphics stack at all, which
is what `examples/alacarte_xr` demonstrates and what makes the rest testable.

With it on, `runtime::OpenXr` is a real session. The `openxr` crate is loaded
at run time rather than linked, so building the feature needs no SDK and running
it on a machine with no runtime answers `runtime::Unavailable::NoRuntime`
rather than failing to start.

`WebXR` is not `OpenXR`: it is a different runtime with a different lifecycle
reached through `web-sys`, and putting it behind the same [`Headset`] trait is a
design question rather than a port. It is not here, and the `openxr` feature is
not built for `wasm32`.

## What the stand-in does not do

A scripted headset exercises the code paths and not the runtime. An `OpenXR`
session that fails to create, a swapchain format the runtime does not offer, a
frame the compositor drops -- none of those happen to a recording, and the only
thing that finds them is a headset in somebody's hands. The stand-in stops the
paths from rotting; it does not certify them.
