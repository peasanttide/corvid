# `corvid_mesh_render`

The device half of a `corvid_mesh::Mesh`: the vertex layout a pipeline names,
the two buffers a draw call needs, and the draw call.

```rust
use corvid_fixed::I16F16;
use corvid_mesh::cube;
use corvid_mesh_render::{VERTEX_LAYOUT, upload};

# fn build(device: &corvid_render::wgpu::Device) {
let uploaded = upload(&cube(I16F16::from_f64(0.5)), device, "cube");
assert_eq!(uploaded.count, 36);
assert_eq!(VERTEX_LAYOUT.array_stride, 12);
# }
```

## Why this is a crate and not two methods

The layout and the upload belong to whichever crate names `wgpu`, and that is
this one. They cannot be inherent items on `Vertex` and `Mesh` -- the orphan rule
does not let a crate add an inherent item to another crate's type -- so they are
a free `const` and a free `fn` here.

That is a small ergonomic loss: `upload(&mesh, device, "cube")` rather than
`mesh.upload(device, "cube")`. It is the price of `corvid_mesh` being usable by
a project that compiles no graphics stack at all, and the alternative was a
crate called `corvid_mesh` that pulls `wgpu` in to build a cube in a test.

## Where `wgpu` comes from

This crate's manifest names `wgpu` directly, as `wgpu = { workspace = true }`,
rather than reaching it through a re-export from `corvid_render` -- `corvid` is
the workspace's one facade, and no crate below it forwards its neighbours.

**The pin is what keeps the version single, not the re-export.** One entry in
the root manifest, one version in the graph, one `raw-window-handle` that a
surface and a window can agree on. So the rule is not "do not name it" but "do
not name a *version* of it".

## What is in a mesh on the device

| | |
|---|---|
| `VERTEX_LAYOUT` | `Snorm16x4` at 0, `Snorm8x2` at 8, stride 12 |
| `Uploaded::vertices` | a `wgpu::Buffer`, `VERTEX` usage |
| `Uploaded::indices` | a `wgpu::Buffer`, `INDEX` usage, `Uint32` |
| `Uploaded::count` | how many indices, which is three per triangle |
| `Uploaded::scale` | `Mesh::scale` as the `f32` a shader multiplies by |

Both attributes arrive at the shader already normalized: the position in
`[-1, 1]` per axis, and the normal as the two octahedral components
`OctDirection` stores. Decoding the second is four lines of WGSL, and
`examples/hello`'s shader has them.

`tests/layout.rs` pins those offsets against the bytes
`corvid_mesh/tests/vertex.rs` freezes, because the two are separate statements
of one fact and neither half alone would notice the other moving.
