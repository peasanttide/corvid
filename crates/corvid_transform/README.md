# `corvid_transform`

Deterministic, integer-only rigid transforms for
[Corvid](https://github.com/peasanttide/corvid), sized for an earth-scale VR
game. Transform points between world and local space using nothing but
fixed-point arithmetic, so every machine running the simulation computes the
same bits.

This crate re-exports [`corvid_fixed`], [`corvid_vector`] and
[`corvid_rotation`], so downstream code depends on one name.

```rust
use corvid_transform::{
    Angle32, FineRotation, FineTransform, GlobalFinePoint, I48F16, Pitch32, Rotation,
    Transform, GlobalPoint, I24F8, Versor,
};

// A head pose on the earth's surface, 6371 km from the origin.
let camera = FineTransform::new(
    GlobalFinePoint::splat(I48F16::from_f64(6_371_000.0)),
    FineRotation::from_versor(Versor::from_yaw_pitch_roll(
        Angle32::from_degrees(30.0),
        Pitch32::from_degrees(-10.0),
        Angle32::ZERO,
    )),
);

// Something a millimetre in front of it.
let target = camera.position() + GlobalFinePoint::new(
    I48F16::from_f64(0.001),
    I48F16::ZERO,
    I48F16::ZERO,
);

// World to eye: the near field still resolves to 15.26 um at that distance
// from the origin, because the subtraction happens before the narrowing.
let local = camera.to_fine_global(target).expect("a millimetre is near field");
assert!(local.length().to_f64() < 0.0011);

// Objects in the world are the coarse tier, at 16 bytes.
let object = Transform::new(
    GlobalPoint::splat(I24F8::from_f64(10.0)),
    Rotation::IDENTITY,
);
assert_eq!(size_of::<Transform>(), 16);
assert_eq!(size_of::<FineTransform>(), 32);
assert_eq!(object.inverse().compose(object), Transform::IDENTITY);
```

## The two tiers

| | Position | Rotation | Size |
|---|---|---|---|
| [`Transform`] | [`GlobalPoint`] — ±8388 km at 3.9 mm | [`Rotation`] — 0.186° | **16 B** |
| [`FineTransform`] | [`GlobalFinePoint`] — ±1.407e14 m at 15.26 µm | [`FineRotation`] — 0.0033° | **32 B** |

Objects in the world are [`Transform`]. The camera and VR tracked poses are
[`FineTransform`]. Both are generated from one macro, so the operation family is
written once and cannot drift between them.

**Both widen to `I48F16` internally.** `Transform`'s own position is a
`GlobalPoint`, so a naive implementation would subtract in `i32` and need a
separate code path with its own overflow story — two `GlobalPoint`s can differ by
more than `GlobalPoint` holds. Widening the operands first is an exact `<< 8`,
makes the subtraction total, and lets both tiers share one macro body. The shift
is free next to the rotation that follows.

## Coordinate convention

Right-handed, **+X right, +Y forward, +Z up**. Yaw about **+Z**, pitch about
**+X**, roll about **+Y**; Euler composition is **ZXY intrinsic**.
`right = forward × up`. Identity faces **+Y** with **+Z** up, so an identity
transform looks forward rather than at the floor. `a.compose(b)` applies **`b`
first**, then `a`.

## World → local, the hot path

```rust
use corvid_transform::{FineTransform, GlobalFinePoint, GlobalPoint, I48F16};

let camera = FineTransform::new(
    GlobalFinePoint::splat(I48F16::from_f64(1.0e13)),
    Default::default(),
);

// The offset is bit-exact before the rotation: both types carry 16 fractional
// bits, so the widen is a shift, the subtract is exact i64, and the narrow is a
// bounds test.
let one_step = camera.position() + GlobalFinePoint::new(
    I48F16::from_bits(1),
    I48F16::ZERO,
    I48F16::ZERO,
);
let local = camera.to_fine_global(one_step).expect("one last bit is near field");
assert_eq!(local.x().to_bits(), 1);

// None appears only when the *offset* leaves range, never for magnitude
// quietly discarded.
let far = GlobalFinePoint::splat(I48F16::from_f64(1.0e13 + 40_000.0));
assert_eq!(camera.to_fine_global(far), None);
assert!(camera.to_local_global(far).is_some());
```

| Method | Argument | Result | Resolution over range |
|---|---|---|---|
| `to_fine` | `GlobalPoint` | `Option<FinePoint>` | 15.26 µm over ±32.7 km |
| `to_fine_global` | `GlobalFinePoint` | `Option<FinePoint>` | 15.26 µm over ±32.7 km |
| `to_local` | `GlobalPoint` | `Option<GlobalPoint>` | 3.9 mm over ±8388 km |
| `to_local_global` | `GlobalFinePoint` | `Option<GlobalPoint>` | 3.9 mm over ±8388 km |
| `to_world` | `FinePoint` | `GlobalFinePoint` | total |
| `to_world_coarse` | `GlobalPoint` | `GlobalFinePoint` | total |

Order inside every `to_*` is **widen → subtract → range-check → narrow →
rotate**. Steps 1–3 into a `FinePoint` are **bit-exact end to end**; nothing
rounds until the rotation, which rounds once. Every unit of error in a
world→eye conversion is attributable to one place.

Narrowing a *difference* rather than an absolute is what makes earth scale work:
the camera can sit 6.37e6 m from the origin — or 1e13 m — and near-field
geometry still resolves to 15.26 µm. **There is no `i128` on this path.**

### Hoist the basis out of hot loops

Every conversion above decodes the packed rotation on the way in, and that
decode dominates the cost. A loop over thousands of points should decode once:

```rust
use corvid_transform::{FineTransform, GlobalFinePoint, I48F16};

let camera = FineTransform::default();
let objects = [GlobalFinePoint::splat(I48F16::from_f64(3.0)); 4];

// Once, not once per point.
let basis = camera.basis();
let origin = camera.origin();
let local: Vec<_> = objects
    .iter()
    .filter_map(|&p| p.checked_sub(origin)?.to_fine())
    .map(|near| basis.unrotate_fine(near))
    .collect();
assert_eq!(local.len(), 4);
```

`examples/earth_scale_vr.rs` measures both forms over 10,000 objects at 90 Hz
with the camera at 6.37e6 m:

| | per point | per frame | of the 11.1 ms budget |
|---|---|---|---|
| `to_fine_global` (decodes per call) | 65.4 ns | 0.65 ms | 5.9% |
| hoisted basis, `i64` local path | **15.7 ns** | **0.16 ms** | **1.4%** |
| hoisted basis, `i128` global path | 22.3 ns | 0.22 ms | 2.0% |

Hoisting is **4.2× faster** — the packed-rotation decode really is the dominant
cost — and subtracting before narrowing buys a further **29%** over rotating at
world width. Even the naive form fits the budget seventeen times over; the point
of the fast path is what is left for everything else in the frame.

## Converting between the tiers

```rust
use corvid_transform::{FineTransform, GlobalFinePoint, I48F16, Transform};

let object = Transform::default();
let fine = object.to_fine_transform();
assert_eq!(fine.to_coarse_transform(), Some(object));

// Range is the only reason the downgrade fails.
let far = FineTransform::new(
    GlobalFinePoint::splat(I48F16::from_f64(1.0e13)),
    Default::default(),
);
assert_eq!(far.to_coarse_transform(), None);
```

`to_fine_transform` cannot fail, but it is **not lossless in the way the name
suggests**: the position widens exactly while the rotation is re-quantized,
adding up to `FineRotation`'s 0.0033° on top of the 0.186° the `Rotation`
already carries. That is a 1.8% increase in a quantity already dominated by the
coarse codec.

`to_coarse_transform` returns `None` **only** on position range; the rotation
always converts, losing accuracy down to the 32-bit tier.

> **Naming note.** The design document called these `to_fine` and `to_coarse`.
> `Transform::to_fine` is already the world→eye point conversion above, and two
> inherent methods cannot share a name, so the tier conversions carry the
> `_transform` suffix. `From` and `TryFrom` impls exist alongside.

## Operation family

`IDENTITY`, `new`, `position`, `rotation`, `basis`, `origin`, `inverse`,
`compose`, `transform_point` / `transform_vector` / `transform_direction` and
their inverses, the six conversions above, `look_at`, `looking_to`,
`looking_at`, `lerp`, `move_towards`, `rotate_towards`, `direction_to`,
`distance_to`, `translated_by`, `rotated_by`, `with_position`, `with_rotation`,
`forward` / `right` / `up`.

## Determinism

Every operation is integer arithmetic and `const`. Floating point appears only
in conversions at the boundary — `from_f64`, `to_f64` and their `f32` forms —
exactly as in `corvid_fixed`. `tests/determinism.rs` compares const-evaluated
results against runtime results: rustc's const interpreter and the CPU are
independent implementations of the same arithmetic, so agreement is evidence,
not tautology.

`tests/vr_stability.rs` turns "does not visibly swim" into three assertions:
bit-identical decoding of a static pose across 10,000 frames, no dither at a
quantization boundary, and bounded steps under a 200°/s sweep sampled at 90 Hz.

## Features

Every integration is optional and off by default.

| Feature | Effect |
|---|---|
| `mint` | `mint` conversions, forwarded from the layers below |
| `nalgebra` | `nalgebra` conversions, forwarded from the layers below |
| `serde` | `Serialize`/`Deserialize`; packed rotations transparently as their integer |
| `bytemuck` | `Pod` and `Zeroable` |
| `arbitrary` | `Arbitrary`, for fuzzing (links `std`) |
| `std` | Forwards `std` to whichever of the above are enabled |

## Out of scope

Transform hierarchies — Corvid has none, by design. Scale, uniform or not: a
transform here is rigid, so the inverse is exact and composition stays in SO(3).
View and projection matrices. Distribution-adaptive rotation codebooks.
