# `corvid_mesh`

Fixed-point geometry with no device in it: a twelve-byte vertex, an indexed
mesh, and six generators that build one.

```rust
use corvid_fixed::I16F16;
use corvid_mesh::{Mesh, cube};

let metre: Mesh = cube(I16F16::from_f64(0.5));
assert_eq!(metre.triangles(), 12);
assert_eq!(metre.bounds().max.x().to_f64(), 0.5);
```

`no_std` plus `alloc`, integer-only, and it names no graphics library at all —
`examples/alacarte_mesh` is a crate whose manifest names this one and nothing
else, and CI builds it for `thumbv7em-none-eabi`. Uploading a mesh is
`corvid_mesh_render`'s job, and it is a separate crate for exactly that reason.

## A vertex is twelve bytes

Three `i16` positions with one uniform scale for the whole mesh, and a
`corvid_vector::OctDirection` normal.

| | Bytes | Read as |
|---|---|---|
| Position | 6, padded to 8 | `Snorm16x4` at offset 0 |
| Normal | 2 | `Snorm8x2` at offset 8 |
| Padding to a four-byte stride | 2 | — |

Against the twenty-four a float vertex costs, and `tests/vertex.rs` freezes the
twelve as bytes rather than as a `size_of`: a struct that kept its size while
its fields moved would pass a size assertion and load every mesh in the world
wrong.

The precision was never the limit. Positions inside a mesh are relative to its
own origin, where sixteen bits over a mesh-sized box is 15 µm on a metre-wide
cube — finer than anything a player can see. What the twelve bytes buy is
memory bandwidth at fifty thousand instances.

There is no three-component sixteen-bit vertex format in WebGPU, so the position
is read as four and the fourth component is padding. That is why the three
fields weigh eight bytes and a vertex buffer costs twelve, and it is documented
on `Vertex` rather than hidden.

## The generators are flat-shaded

| | |
|---|---|
| `cube(half)` | 12 triangles |
| `quad(half)` | 2, facing +Z |
| `grid(half, cells)` | `2·cells²`, facing +Z |
| `icosphere(radius, subdivisions)` | `20·4^subdivisions`, poles on ±Z |
| `cylinder(radius, half_height, sides)` | `4·sides`, closed |
| `cone(radius, half_height, sides)` | `2·sides`, closed, apex at +Z |

Every one emits one vertex per face corner rather than sharing corners between
faces, because a device has no per-face storage: a normal is a vertex attribute
or it is nothing. A caller that wants smooth normals is generating a different
mesh, not post-processing one of these.

Four properties hold for all six, and `tests/shapes.rs` checks each of them
against each generator, because they are the four that catch a generator that is
wrong rather than merely different:

- every index names a vertex that exists;
- every face is wound counter-clockwise seen from outside;
- every vertex's stored normal agrees with the face it belongs to, which is what
  flat-shaded means and is what a shared-corner mistake breaks;
- the extremes reach `±Vertex::FULL`, so the mesh fills the box its `scale`
  claims rather than sitting inside it.

The last one is why `icosphere` puts its poles on ±Z. The usual golden-ratio
orientation of an icosahedron has no vertex on any axis, so it would leave 15%
of every position component unused.

## Bounds

`Mesh::bounds` is in **metres**, and it is the one place the two scales meet: a
`Vertex` position is a signed fraction of `Vertex::FULL` and a
`corvid_shape::Aabb` is in `GlobalPoint` metres, so the conversion multiplies by
`Mesh::scale`. A mesh with no vertices bounds nothing, which is `Aabb::EMPTY`
rather than a point at the origin — the difference between "nothing to draw" and
"one degenerate thing at the world's centre".

## Feature gating

`bytemuck` is **on by default here**, where it is optional everywhere else in
this workspace. A vertex whose whole purpose is to become the bytes of a vertex
buffer and cannot is not useful, and `bytemuck` is `no_std`, so nothing is paid
for it on a target with no operating system.
