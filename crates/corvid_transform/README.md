# `corvid_transform`

Deterministic, integer-only rigid transforms for Corvid, sized for an earth-scale
VR game. `no_std`, every operation `const`. This crate re-exports
[`corvid_fixed`], [`corvid_vector`] and [`corvid_rotation`], so downstream code
depends on one name.

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

// Something a millimetre in front of it. World to eye still resolves to
// 15.26 um at that distance, because the subtraction happens before the
// narrowing.
let target = camera.position() + GlobalFinePoint::new(
    I48F16::from_f64(0.001),
    I48F16::ZERO,
    I48F16::ZERO,
);
let local = camera.to_fine_global(target).expect("a millimetre is near field");
assert!(local.length().to_f64() < 0.0011);

// Objects in the world are the coarse tier, at 16 bytes.
let object = Transform::new(
    GlobalPoint::splat(I24F8::from_f64(10.0)),
    Rotation::IDENTITY,
);
assert_eq!(size_of::<Transform>(), 16);
assert_eq!(object.inverse().compose(object), Transform::IDENTITY);
```

Two tiers, generated from one macro so the operation family cannot drift between
them. [`Transform`] is 16 bytes: a [`GlobalPoint`] position at 3.9 mm over
+/-8388 km, and a [`Rotation`]. [`FineTransform`] is 32 bytes: a
[`GlobalFinePoint`] at 15.26 um over +/-1.407e14 m, and a [`FineRotation`].
Objects in the world are the first; the camera and VR tracked poses are the
second. Both widen to `I48F16` internally, which is an exact shift and makes the
subtraction total, so two coarse positions differing by more than `GlobalPoint`
holds still subtract correctly.

The convention is right-handed, **+X right, +Y forward, +Z up**, with yaw about
+Z, pitch about +X and roll about +Y composed ZXY intrinsic. Identity faces +Y
with +Z up, so an identity transform looks forward rather than at the floor.
`a.compose(b)` applies `b` first.

World to local is the hot path, and its order is **widen, subtract, range-check,
narrow, rotate**. The first three steps are bit-exact end to end and nothing
rounds until the rotation, which rounds once, so every unit of error in a
world-to-eye conversion is attributable to one place. Narrowing a difference
rather than an absolute is what makes earth scale work: the camera can sit
6.37e6 m from the origin, or 1e13 m, and near-field geometry still resolves to
15.26 um -- with no `i128` on the path. `None` appears only when the *offset*
leaves range, never for magnitude quietly discarded.

Every conversion decodes the packed rotation on the way in, and that decode
dominates the cost, so a loop over thousands of points should decode once:

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

Converting between the tiers is [`to_fine_transform`](Transform::to_fine_transform)
and [`to_coarse_transform`](FineTransform::to_coarse_transform). The upgrade
cannot fail but is not lossless in the way the name suggests: the position widens
exactly while the rotation is re-quantized. The downgrade returns `None` only on
position range; the rotation always converts, losing accuracy to the 32-bit tier.

The optional integrations are `mint`, `nalgebra`, `serde`, `bytemuck` and
`arbitrary`, all off by default and forwarded to the layers below.

## Scope

Rigid transforms, which means no scale, uniform or otherwise: the inverse is
exact and composition stays in SO(3). No transform hierarchies -- Corvid has
none, by design -- and no view or projection matrices, which take a fixed-point
type on their near side and belong to the crate that owns the camera.
