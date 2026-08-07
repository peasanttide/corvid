# `corvid_rotation`

Deterministic fixed-point SO(3) for
[Corvid](https://github.com/peasanttide/corvid): two packed rotation codecs
sized for network traffic, and two working types sized for arithmetic. Every
operation is `const` and integer-only, so the same inputs give the same bits on
every machine running the simulation.

```rust
use corvid_fixed::{Angle32, Pitch32};
use corvid_rotation::{Basis, FineRotation, Rotation, Versor};

// Identity faces +Y with +Z up, so an identity transform looks forward
// rather than at the floor.
let m = Basis::IDENTITY;
assert_eq!(m.forward().y().to_f64(), 1.0);
assert_eq!(m.up().z().to_f64(), 1.0);

// Yaw about +Z, pitch about +X, roll about +Y; ZXY intrinsic.
let pose = Basis::from_yaw_pitch_roll(
    Angle32::from_degrees(90.0),
    Pitch32::ZERO,
    Angle32::ZERO,
);
// A quarter turn of yaw takes forward (+Y) onto left (-X).
assert!((pose.forward().x().to_f64() + 1.0).abs() < 1e-4);

// Packing costs accuracy, and the tiers say how much.
let packed = FineRotation::from_basis(pose);
assert!(packed.to_versor().angle_to(pose.to_versor_const()).to_degrees() < 1.0 / 128.0);

// A rotation and its double-cover twin are one value.
let q = Versor::from_axis_angle(m.up(), Angle32::from_degrees(30.0));
assert_eq!(FineRotation::from_versor(q), FineRotation::from_versor(q.negate()));
```

## Coordinate convention

Right-handed, **+X right, +Y forward, +Z up**.

- Yaw rotates about **+Z**, pitch about **+X**, roll about **+Y**.
- Euler composition is **ZXY intrinsic** — yaw, then pitch, then roll.
- `right = forward × up`, consistent with `X × Y = Z`.
- Identity faces **+Y** with **+Z** up.
- `a.compose(b)` applies **`b` first**, then `a` — matrix multiplication order,
  and `glam`'s `Mul`.

## The four types

| Type | Size | Role |
|---|---|---|
| [`Rotation`] | 4 B | Packed, 32-bit tier: 0.0784° mean, 0.1832° max |
| [`FineRotation`] | 8 B | Packed, 64-bit tier: 0.0017° mean, 0.0033° max |
| [`Basis`] | 36 B | Working 3×3 matrix; rotating many points, free inverse |
| [`Versor`] | 16 B | Working unit quaternion; cheap composition |

A packed form decodes to the same bits every time it is asked, and packing an
unmodified decode is stationary: a first repack can land one step over, because
both encoders round after normalizing and a lattice point is not always the
closest lattice point to its own normalized direction, but it goes no further.
So the frame loop of unpack, use and repack has nowhere to drift to, and a
working type rebuilt from a packed one starts where it started rather than where
the last frame left it: anything long-lived is stored as a [`Rotation`] or a
[`FineRotation`] and unpacked on use. `tests/determinism.rs` pins both halves,
for both tiers and through both working types.

### `Rotation` — 32 bits, Gibbs linear 2+10+10+10

A 2-bit chart index selects the largest-magnitude quaternion component; the
other three are divided by it, giving the Gibbs vector `t = tan(θ/2)·axis`,
which lies in exactly the cube `[-1, 1]³`. Three 10-bit fields store `t`.

**0.0784° mean, 0.1832° max** over uniform SO(3) — inside the 1/5° budget, and
the cheapest decode in the family. Alternatives measured and rejected, all
figures from `examples/rotation_quality.rs` over 200,000 uniform samples with an
`f64` reference and the chord metric:

| codec | mean | max | decode work beyond the shared normalize |
|---|---|---|---|
| **gibbs linear 2+10+10+10** | 0.0784° | **0.1832°** | none |
| gibbs bcc linear 2+1+29 | 0.0766° | 0.1528° | 2 int div/mod by N=812 |
| smallest-three (baseline) | 0.0844° | 0.2423° | — misses the budget |

Every rejected codec performs the same normalize and then strictly more work, so
the ranking holds regardless of what the integer costs turn out to be. The BCC
variant buys 17% of the worst case for two integer divisions and two modulos per
decode; the crate declines that trade because the budget is already met.

The angular metric is the chord form `4·asin(chord/2)`, never `2·acos(|q₁·q₂|)`
— the `acos` form has a noise floor that at `FineRotation`'s error would be
measuring the harness rather than the codec.

### `FineRotation` — 64 bits, 4×`Signed16` quaternion

Four SNORM components, no chart and no warp: **0.0017° mean, 0.0033° max**,
against a 1/128° (0.0078°) budget. At 64 bits the chart machinery stops paying
for itself: the redundancy of four numbers for three degrees of
freedom costs about one bit, and the budget is there.

The largest-magnitude component is forced positive, ties broken by lowest
index. Without this the double cover gives one rotation two bit patterns and
`Hash` and `Eq` would lie.

### `Basis` and `Versor`

`Basis` rotates a point in 9 multiplies, 6 adds and 3 shifts, and its inverse is
the transpose — so untransforming costs exactly what transforming costs. That
wins when many points go through one rotation, which is the earth-scale VR case.
`Versor` composes more cheaply and is 44% of the size, but goes through the
matrix form to rotate a point.

Measured by `examples/rotation_bench.rs`, against an `f32` matrix baseline that says what
determinism costs:

| operation | `f32` matrix | `Basis` | `Versor` |
|---|---|---|---|
| rotate a point | 1.93 ns | **12.6 ns** | 38.5 ns |
| unrotate a point | — | 12.2 ns | — |
| compose | 4.80 ns | 35.6 ns | **17.6 ns** |

**Compose as a versor, rotate as a basis.** Determinism costs about 6.5× on the
rotation and 3.7× on the composition.

Every column computes the *whole* result. That is worth saying because it was
once not true: consuming a single component let the optimizer delete the work
behind the other two — eight of a compose's nine entries — and understated the
fixed-point rows by up to 6.9×, the `f32` baselines along with them.

Packing and unpacking are dominated by the shared normalize — 28 ns to pack and
49 ns to unpack a `Rotation` — which is why they belong once per frame, not once
per point.

**`nlerp` is the default, `slerp` is opt-in.** True slerp needs `acos` and
`sin`, both on `corvid_fixed`'s CORDIC path where the crate's slowest functions
live. `nlerp` is a lerp plus one normalize, and over the few degrees a frame
actually spans its departure from constant angular velocity is not observable.

## Every interpolation here returns its endpoints unchanged

`nlerp`, `slerp` and `rotate_towards` hand back the rotation they were given at
a weight of `ZERO`, at a weight of `ONE`, and — for `rotate_towards` — on a step
of no angle at all and on a step that covers the whole gap. Bit for bit, and
including the sign the caller passed a versor in with, since a versor and its
negation name one rotation and are two values.

That is a short-circuit rather than a property of the arithmetic, and the
arithmetic is why it has to be. A `normalize4` moves a component of an
already-unit versor about half the time, and `Versor::nlerp` at `ZERO` is a mix
in proportion zero — which is the versor it started from — followed by exactly
that normalize. So the drift the short-circuit hides is still measurable without
unpicking anything: `renormalize` is that same `normalize4` and nothing else,
and `examples/rotation_quality.rs` calls it on a million uniform versors. It
moves **499727** of them. `Basis` loses a conversion at each end on top of that
and moves **855716**.

The rotation is never wrong by more than a representation. The largest gap over
that million is **0.00000083°** by the chord form `4·asin(chord/2)` in `f64`,
about a four-thousandth of the fine codec's own 0.0033° quantum. It matters
anyway, because a golden capture compares bytes and "within a quantum" and
"equal" are different answers to that comparison. `tests/ops.rs` walks twenty
thousand random pairs per interpolation.

### A zero step is a no-op, and measuring cannot make it one

`rotate_towards` asks whether the remaining angle fits inside the step. That
question goes through `angle_to`, an `acos`, which is ill-conditioned at `1` and
reports a flat zero for any pair closer than about 0.0025°. Two rotations inside
that resolution therefore satisfy `remaining <= max_step` at a `max_step` of
`ZERO` — and a guard that answers *that* by returning the target has moved a
caller who asked to stand still, to a rotation that was never theirs.

So the zero step is decided before the angle is measured, and at each tier
rather than only at the versor: `Basis::rotate_towards` picks its answer by
comparing versors, and about one in five hundred neighbouring matrix pairs share
one, which is the same failure with the measurement removed. `tests/ops.rs`
builds its pairs by nudging a rotation a fraction of a degree instead of drawing
two, because two uniform rotations are most of a turn apart and never reach
either blind spot.

## `Basis` cannot be built from arbitrary entries

Rotating a `FinePoint` by an `I2F30` basis row is `i32 × i32 → i64`:

| entry type | worst-case row sum | vs `i64::MAX` |
|---|---|---|
| `I2F30` (chosen) | `√3 × 2^30 × 2^31` = 3.99e18 | **131% margin** |
| `Signed32` (rejected) | `√3 × (2^31−1) × 2^31` = 7.99e18 | 15% margin |

The bound is Cauchy–Schwarz — `|m·v| ≤ |m||v|` with `|m| = 1` — and it holds
**only because basis rows are unit-length**. Partial sums obey the same bound
with `√2` in place of `√3`, so there is no ordering hazard and no need to fix an
accumulation order.

A row longer than one lifts that sum past `i64::MAX`, so **the ordinary way in
is a type already known to be a rotation.** [`Basis::from_rows`], which exists
for deserialization and FFI, verifies orthonormality and a determinant of `+1`
first:

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

// The identity survives.
assert_eq!(
    Basis::from_rows([[one, zero, zero], [zero, one, zero], [zero, zero, one]]),
    Some(Basis::IDENTITY)
);
```

`Versor::from_xyzw` gets the same treatment against unit norm, for the same
reason: a non-unit versor produces a non-orthonormal `Basis`.

The `bytemuck` feature is the exception, and it is deliberate: `Pod` makes any
36 bytes a `Basis` and any 16 a `Versor`, bypassing both checks. Bytes that did
not come from this crate should go through `Basis::from_rows`/
`Versor::from_xyzw`, or through [`Rotation`]/[`FineRotation`], whose every bit
pattern is a valid rotation by construction — a forged `Basis` with rows longer
than one breaks the `i64` bound above.

Rotating a `GlobalFinePoint` directly is `i64 × i32 → i128` and is the
documented slow path. The fast pattern subtracts first, which is what
`corvid_transform`'s world→local conversions do.

## Where `Option` appears

Only on genuinely degenerate input: `look_to` with forward parallel to up or
either vector zero-length, and the checked constructors above. Everything else
is total, as the workspace's `panic = "deny"` requires.

## Features

Every integration is optional and off by default.

| Feature | Effect |
|---|---|
| `mint` | `From`/`Into` for `mint::Quaternion` |
| `nalgebra` | Conversions to `Matrix3` and `UnitQuaternion` |
| `serde` | `Serialize`/`Deserialize`; the packed types transparently as their integer |
| `bytemuck` | `Pod` and `Zeroable` — see the note below |
| `arbitrary` | `Arbitrary`, for fuzzing (links `std`) |
| `std` | Forwards `std` to whichever of the above are enabled |

`Hash` absorbs what each type compares by, which differs across the four for
the reasons `Eq` already differs. A [`FineRotation`] folds the double cover, so
a pattern that arrived with the other sign marks as the rotation it denotes; a
[`Rotation`] does not, because folding it costs a decode and a re-encode and its
`Eq` declines to pay that too. A [`Versor`] and a [`Basis`] absorb their
components as stored, and a versor and its negation are two values there — which
is another reason a long-lived orientation belongs in a packed type, where the
same rotation encodes to the same bits and a versor's negation encodes to those
same bits too.

Encoding is the operative word, and for a [`Rotation`] it narrows the problem
rather than removing it. All `2³²` patterns are valid rotations, and 0.58% of
arbitrary ones re-encode to bits other than the ones they came in as, which
`from_bits`, `serde`, `bytemuck` and `arbitrary` each hand through untouched.
Restricting to what the encoder itself produced drops that to 0.065%, not to
zero: where two quaternion components tie in magnitude either can serve as the
chart, and a re-encode picks the other one. Two peers can then hold one rotation
as two patterns, compare unequal, and exchange marks that disagree — the same
divergence this section warns about, arriving through the other door. So an
orientation that came in raw wants `canonicalize` at the boundary, which is
where the decode and re-encode is paid once instead of on every comparison for
the rest of that value's life. Both figures are measured in
`tests/rotation32.rs`.

Whether the residual 0.065% can bite a lockstep simulation depends on how the
two peers got there. Peers running the same operations on the same inputs reach
the same bits, ties included, because the tie-break is deterministic; the risk
is two peers reaching one orientation by different routes — one from a decoded
network pattern, one from its own composition — and then comparing or marking.
That is the same boundary `canonicalize` already guards.

`serde` on the packed rotations serializing as a bare `u32`/`u64` is what makes
`corvid_transform`'s 16 B and 32 B figures mean something over the wire, so
`tests/interop.rs` asserts the serialized form rather than only that a round
trip succeeds.
