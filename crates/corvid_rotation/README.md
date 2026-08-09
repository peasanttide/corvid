# `corvid_rotation`

Deterministic fixed-point SO(3) for Corvid: two packed codecs sized for network
traffic, and two working types sized for arithmetic. `no_std`, every operation
`const` and integer-only.

```rust
use corvid_fixed::{Angle32, Pitch32};
use corvid_rotation::{Basis, FineRotation, Rotation, Versor};

// Identity faces +Y with +Z up, so an identity transform looks forward
// rather than at the floor.
let m = Basis::IDENTITY;
assert_eq!(m.forward().y().to_f64(), 1.0);
assert_eq!(m.up().z().to_f64(), 1.0);

// Yaw about +Z, pitch about +X, roll about +Y; ZXY intrinsic. A quarter turn
// of yaw takes forward (+Y) onto left (-X).
let pose = Basis::from_yaw_pitch_roll(
    Angle32::from_degrees(90.0),
    Pitch32::ZERO,
    Angle32::ZERO,
);
assert!((pose.forward().x().to_f64() + 1.0).abs() < 1e-4);

// A rotation and its double-cover twin are one value.
let q = Versor::from_axis_angle(m.up(), Angle32::from_degrees(30.0));
assert_eq!(FineRotation::from_versor(q), FineRotation::from_versor(q.negate()));
```

The convention is right-handed, **+X right, +Y forward, +Z up**: yaw about +Z,
pitch about +X, roll about +Y, composed ZXY intrinsic, with `right = forward x
up`. `a.compose(b)` applies `b` first.

| Type | Size | Role |
|---|---|---|
| [`Rotation`] | 4 B | Packed, 32-bit tier: 0.18 deg worst case |
| [`FineRotation`] | 8 B | Packed, 64-bit tier: 0.0033 deg worst case |
| [`Basis`] | 36 B | Working 3x3 matrix; rotating many points, free inverse |
| [`Versor`] | 16 B | Working unit quaternion; cheap composition |

[`Rotation`] is a 2-bit chart index naming the largest quaternion component and
three 10-bit fields holding the Gibbs vector `t = tan(theta/2)*axis`, which lies
in exactly the cube `[-1, 1]^3`. [`FineRotation`] is four SNORM components with
no chart at all, since at 64 bits the chart machinery stops paying for itself.
Both are canonical -- the largest component is forced positive -- so a rotation
has one bit pattern and `Hash` and `Eq` do not lie, and both round-trip without
drift, which is why anything long-lived is stored packed.

Compose as a [`Versor`] and rotate as a [`Basis`]. A basis rotates a point in
nine multiplies, six adds and three shifts, and its inverse is the transpose, so
untransforming costs what transforming costs; a versor composes more cheaply and
is 44% of the size but goes through the matrix form to rotate anything. Packing
and unpacking are dominated by a shared normalize, so they belong once per frame
rather than once per point. Interpolation is `nlerp` by default with `slerp`
opt-in, because true slerp needs the `acos` and `sin` on `corvid_fixed`'s
slowest path and over the few degrees a frame spans the difference is not
observable.

A [`Basis`] cannot be built from arbitrary entries. Rotating a `FinePoint` by an
`I2F30` row is `i32 x i32 -> i64`, and the bound that keeps the row sum inside
`i64` is Cauchy-Schwarz with `|m| = 1` -- it holds only because the rows are
unit-length. So the ordinary way in is a type already known to be a rotation,
and [`Basis::from_rows`], which exists for deserialization and FFI, verifies
orthonormality and a determinant of `+1` first:

```rust
use corvid_fixed::I2F30;
use corvid_rotation::Basis;

let one = I2F30::ONE;
let zero = I2F30::ZERO;

// A scaled row is exactly the case that would break the `i64` bound.
let scaled = I2F30::from_f64(1.9);
assert_eq!(
    Basis::from_rows([[scaled, zero, zero], [zero, one, zero], [zero, zero, one]]),
    None
);

// So is a reflection: orthonormal, but determinant -1.
assert_eq!(
    Basis::from_rows([
        [one, zero, zero],
        [zero, one, zero],
        [zero, zero, I2F30::from_f64(-1.0)],
    ]),
    None
);

assert_eq!(
    Basis::from_rows([[one, zero, zero], [zero, one, zero], [zero, zero, one]]),
    Some(Basis::IDENTITY)
);
```

[`Versor::from_xyzw`] gets the same treatment against unit norm. The `bytemuck`
feature bypasses both, deliberately, so bytes that did not come from this crate
should arrive through a checked constructor or through a packed type, whose
every bit pattern is a valid rotation by construction.

`Option` appears only on genuinely degenerate input: `look_to` with forward
parallel to up or either vector zero-length, and the checked constructors.
Everything else is total. The optional integrations are `mint`, `nalgebra`,
`serde`, `bytemuck` and `arbitrary`, all off by default.

## Scope

SO(3), and nothing around it. No scale, uniform or otherwise, and no translation
-- a rigid transform is `corvid_transform`, which is these rotations with a
position beside them. No Euler-angle type either: yaw, pitch and roll go in and
come back out as [`Angle32`](corvid_fixed::Angle32) and
[`Pitch32`](corvid_fixed::Pitch32).

Two packed tiers and two working types. A third tier arrives when a budget
between them does. Distribution-adaptive codebooks are out: they buy accuracy
back by assuming what the rotations look like, and a framework does not know
that.
