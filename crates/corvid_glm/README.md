# `corvid_glm`

The `f32` vectors and matrices a [Corvid](https://github.com/peasanttide/corvid)
game hands its device. `no_std`.

```rust
use corvid_glm::{Mat4, Vec3};

let axis = Vec3::new(0.0, 0.0, 1.0);
let identity = Mat4::identity();

assert_eq!(identity * axis.to_homogeneous(), axis.to_homogeneous());
```

## Column-major, which is what a shader reads

WGSL's `mat4x4` is column-major and so is nalgebra's `Matrix4`, so a matrix
here is already in the order a uniform buffer wants. There is no transpose on
the way to the device.

That is a claim about byte *order* and not about alignment, and the two are
worth keeping apart. `Mat4` is sixty-four bytes aligned to four; a WGSL matrix
is aligned to sixteen, and so are `vec3` and `vec4` against this crate's `Vec3`
and `Vec4`. Written at an offset that is already sixteen-aligned -- the start of
a buffer, or a binding of its own -- the difference cannot be observed, which is
why the ordinary case just works. Put one inside a `#[repr(C)]` struct and cast
the struct to bytes and it can be: Rust places the field on four, the shader
reads it on sixteen, and they disagree from the first field that does not land
on both. A struct that crosses to a shader owes its own padding, and that
padding belongs to the game, because only the game knows what else is in it.

`Matrix4::new` takes its arguments row by row and stores them column by column,
so a matrix can still be *written* in reading order:

```rust
use corvid_glm::Mat4;

// Written across; stored down.
const FLIP: Mat4 = Mat4::new(
    1.0, 0.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.0, 1.0,
);
```

It is `const`, which is what lets a projection be one.

## Features

Every integration is optional and off by default.

| Feature | Effect |
|---|---|
| `bytemuck` | `Pod` and `Zeroable`, so a matrix reaches a mapped buffer without an `unsafe` block |
| `mint` | `From`/`Into` for the `mint` vector and column-matrix types |
| `std` | Forwards `std` to nalgebra and bytemuck; adds nothing on this side |

The impls behind the first two are nalgebra's own, switched on through its
`convert-bytemuck` and `mint` features. This crate writes neither, and could
not have: the workspace forbids `unsafe_code`.

The crate is `no_std` under every feature, `std` included -- the inner attribute
is unconditional, because type aliases and a `const` have nothing an allocator
could be needed for. `std` exists so a downstream that is already linking it can
say so to the graph underneath, rather than leave nalgebra and bytemuck in the
`no_std` configuration the default build picks.

## What belongs here

Types, and one value. `IDENTITY` is the 4x4 identity as a `const`, which
`Matrix4::identity` cannot be -- it is a function, so a default camera or a model
matrix a game overwrites per instance would otherwise have to be built at
runtime.

Nothing beyond that. The matrices a camera is turned into live in
`corvid_camera`, because every one of them takes a fixed-point Corvid type on
its near side and this crate has no opinion about fixed point. `corvid_float`
is the scalar half.
