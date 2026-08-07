# `corvid_glm`

The `f32` vectors and matrices a [Corvid](https://github.com/peasanttide/corvid)
game hands its device. `no_std`.

There is no linear algebra in this crate. It is
[nalgebra](https://crates.io/crates/nalgebra), pinned once for the whole
workspace, with the names a game spells its types by:

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

That is worth stating because the workspace used to do the opposite: two
crates each declared their own `pub type Mat4 = [[f32; 4]; 4]`, documented as
row-major, and each carried a `columns()` that transposed on the way out. One
convention and one type replaces both.

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

## What belongs here

Types, and nothing else. The matrices a camera is turned into live in
`corvid_camera`, because every one of them takes a fixed-point Corvid type on
its near side and this crate has no opinion about fixed point. `corvid_float`
is the scalar half.
