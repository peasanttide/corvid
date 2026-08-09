# `corvid_glm`

The `f32` vectors and matrices a Corvid game hands its device: nalgebra, in the
order a shader reads, under the names a game spells them by. `no_std`.

```rust
use corvid_glm::{Mat4, Vec3};

let axis = Vec3::new(0.0, 0.0, 1.0);
let identity = Mat4::identity();

assert_eq!(identity * axis.to_homogeneous(), axis.to_homogeneous());
```

[`Vec2`], [`Vec3`], [`Vec4`], [`Mat3`] and [`Mat4`] are aliases for nalgebra's
`f32` types, and [`IDENTITY`] is the 4x4 identity as a `const`, which
`Matrix4::identity` cannot be. `Matrix4::new` takes its arguments row by row and
stores them column by column, so a matrix is still written in reading order:

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

WGSL's `mat4x4` is column-major and so is nalgebra's `Matrix4`, so nothing is
transposed on the way to a uniform buffer. That is a claim about byte order and
not about alignment: a `Mat4` is sixty-four bytes aligned to four where a WGSL
matrix is aligned to sixteen, which cannot be observed at an offset that is
already sixteen-aligned and can be inside a `#[repr(C)]` struct cast to bytes. A
struct that crosses to a shader owes its own padding, because only the game
knows what else is in it.

The optional `bytemuck` and `mint` features switch on nalgebra's own impls; this
crate writes neither, and could not, since the workspace forbids `unsafe`.

## Scope

Types, and one value. Nothing here computes: the matrices a camera is turned
into belong to the crate that owns the camera. `corvid_float` is the scalar
half.
