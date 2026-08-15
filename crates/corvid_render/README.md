# `corvid_render`

The `wgpu` half of Corvid: a device, a target, and a trait a game records its
own frame through.

There is no draw-list type here and no command vocabulary. A game that draws
gets a real `wgpu::Device` and writes real `wgpu`. Every abstraction over a GPU
is a bet about which games exist, and this one is not in that business.

```rust
use core::convert::Infallible;

use corvid_behavior::{Extract, Extracting, Level, State};
use corvid_render::{Drawing, Opened, Render};
# use serde::{Deserialize, Serialize};
# #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
# struct Field;
# impl Level for Field {
#     type Error = Infallible;
#     fn load(_: &str) -> Result<Self, Infallible> { Ok(Self) }
# }
# #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
# struct Game;
# impl State for Game {
#     const NAME: &'static str = "game";
#     type Level = Field; type Rules = (); type Action = ();
# }

/// The renderer *is* the graphics: whatever pipelines and buffers it needs are
/// its own fields. This one clears, so it has none.
struct Clear;

impl Extract<Game> for Clear {
    /// At most once per displayed frame, for the settled newest state. This is
    /// where a renderer writes the pair its shader will lerp between.
    fn extract(&mut self, _extracting: Extracting<'_, Game>) {}
}

impl Render<Game> for Clear {
    type Config = ();

    fn new(_opened: Opened<'_>, (): ()) -> Self {
        Self
    }

    fn configure(&mut self, (): ()) {}

    fn draw(&mut self, drawing: Drawing<'_>, encoder: &mut wgpu::CommandEncoder) {
        // A real encoder, a real texture view, a real device and queue. Begin
        // as many passes as the frame wants; nothing here is a wrapper.
        let target = drawing.target;
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
}
```


`Render` is the half of a game's client-local code that knows there is a device.
It is one of the independent types a `Game` is made of -- beside a `State`, a
`Controller`, a `Bot` and an `Auralizer` -- rather than a link in a chain over
one marker type.

That independence is the point. A renderer implements `Render<S>` for **its
own** type, so an art crate can write one against a simulation crate's state
with nothing in between. And there is no `Graphics` associated type and no
`Setup` trait: `Self` holds the pipelines, so the two collapse into `new`.

It is that way round so that the other way round is possible. A dedicated
server, a determinism check in CI and a game that has not drawn anything yet all
implement the first two and stop -- and a build that stops there has no `wgpu` in
it at all, because nothing it names lives in this crate. `corvid_app`'s `render`
feature is where that is chosen, and its `tests/graphicless.rs` is where the
dependency graph is asked rather than assumed.

The two types are here rather than a crate up because `draw` takes one of each,
and the base of a chain is the place everything above can name. A game that
draws nothing still implements this trait -- `type Graphics = ();` writes
all four items -- because there is no build in which the runtime cannot draw.

## What this crate owns, and what it refuses to

It owns the device, the target, the frame's acquire-record-submit-present step,
resize, the readback that makes a capture possible, and the camera's GPU form.
It owns no pipeline, no shader, no material, no light, no shadow, no scene
graph, no pass graph and no opinion about what a frame contains. There is
nothing in `Renderer` that knows what geometry is.

**It owns the device, and it re-exports `wgpu` for a game that reaches this
crate directly.** What keeps one `wgpu` and one `raw-window-handle` in the graph
is the workspace pin rather than that re-export: `wgpu = { workspace = true }`
in every manifest that needs it resolves to the single entry in the root, and
`corvid_ui_render`'s `tests/manifest.rs` is where that is checked rather than
merely stated.

The arithmetic at the fixed-to-floating boundary is **not** here. It is
`corvid_camera::matrix`, re-exported above, and it lives there because it is
*maths* rather than a rendering decision: every function in it takes a
fixed-point Corvid type on its near side and none of them takes a `wgpu` one,
so the whole boundary is testable in a crate that has never heard of a device.
A game writing its own shader still has to answer "which way is up in clip
space", and getting that wrong puts a picture on the screen upside down rather
than failing. The camera convention is the workspace's -- **+X right, +Y
forward, +Z up**, right-handed -- and the clip convention is `wgpu`'s;
`corvid_camera`'s `tests/matrix.rs` is what pins the swap between them,
including which way is up and which way is far.

A `Mat4` is `corvid_glm`'s, which is nalgebra's, which is **column-major** --
the order a WGSL `mat4x4` reads. Nothing transposes on the way to the device.

`matrix::model` subtracts the eye before anything reaches `f32`. A
`GlobalFinePoint` reaches 1.4e14 m and an `f32` has twenty-four bits of
mantissa, so a position converted directly would quantize to metres at planetary
distance. Converting the *difference* means the precision follows the camera.

## The eye

`Eye::new` builds the whole camera as the bytes a uniform buffer takes:

| Field | Type | Offset | What it is |
|---|---|---|---|
| `coarse` | `[i32; 3]` | 0 | whole metres, integer and exact |
| `_pad` | `i32` | 12 | the fourth word a `vec3<i32>` occupies anyway |
| `clip` | `Mat4` | 16 | view x projection, relative to `coarse` |

```rust
use std::mem::{align_of, offset_of, size_of};

use corvid_camera::Eye;

// What a uniform buffer is written from, so the layout is the contract rather
// than an implementation detail.
assert_eq!(size_of::<Eye>(), 80);
assert_eq!(align_of::<Eye>(), 4);
assert_eq!(offset_of!(Eye, coarse), 0);
assert_eq!(offset_of!(Eye, _pad), 12);
assert_eq!(offset_of!(Eye, clip), 16);
```

The split is what `matrix::model` does per instance, done once per frame and
handed to the shader instead. A game subtracts `coarse` from a world position in
integers -- where it is exact and free -- and multiplies the difference by `clip`,
so every `f32` sees a difference rather than an absolute.

## Geometry is somewhere else

`Vertex` and `Mesh` are `corvid_mesh`'s, which is `no_std` and names no graphics
library at all; the vertex layout, the upload and the draw call are
`corvid_mesh_render`'s, which takes `wgpu` from the same workspace pin.
`tests/offscreen.rs` draws a `corvid_mesh` cube through this renderer, which is
what says the three fit together.

The normal in that vertex is an integer type from the maths stack, laid out as
`Snorm8x2` on purpose, so a shader reads a `vec2<f32>` and decodes it in four
lines. `examples/hello`'s shader has those four lines and
`tests/offscreen.rs::the_normal_reaches_the_shader_and_is_decoded_there` is what
says a device agrees with the encoder.

## Two targets, one renderer

| Constructor | Draws into | Read back |
|---|---|---|
| `Renderer::for_window` | a window's surface | no -- a presented frame belongs to the compositor |
| `Renderer::offscreen` | a texture | `read_back` gives RGBA8 rows, and `Image::to_png` a file |

The same device, the same `Render` implementation and the same recording run
against either. That is what makes a headless frame the *same* frame rather than
a second implementation of one, and it is what lets the whole path be exercised
on a machine with no display: `tests/offscreen.rs` draws a fixed-point cube
through it and reads the pixels back. Those tests need an adapter -- a real GPU
or a software rasteriser such as Mesa's `lavapipe` -- and report that they were
skipped, by name, on a machine that has neither.

## What a captured frame proves, and what it does not

Raw `wgpu` cannot be diffed: there is no serializable record of *what the game
asked for*. So the capture seam is a PNG read back off the offscreen texture,
and it is worth being exact about how weak a golden that is.

A PNG is a **perceptual** golden. Rasterisation differs between drivers, so it
is compared with a tolerance -- `corvid_test::Tolerance` -- and its exact-match
arm is pinned to the software adapter, which `adapter_is_software` is how a test
asks about. `tests/offscreen.rs::the_same_frame_twice_is_the_same_bytes_and_survives_a_png`
is what makes that arm worth anything: one adapter drawing one frame twice
produces one answer, so a byte that moved between two runs on one machine moved
for a reason. It says nothing about two different adapters.

A comparison at a tolerance proves the frame is *about* the same picture. It
does not prove the frame is the one the game meant to draw, and no screenshot
comparison can: the right geometry in the wrong colour passes anything loose
enough to survive two drivers.

The **bit-exact** golden is the state hash trace, compared byte for byte on
every target, and it is the one that matters: a picture that agreed while the
simulation diverged would say nothing at all.

## Safety

`unsafe_code` is forbidden here, as everywhere in this workspace. Surface
creation is `wgpu::Instance::create_surface`, which is the
safe constructor: it takes a window handle by value and keeps it alive for the
surface's life, so the lifetime obligation `create_surface_unsafe` puts on the
caller is discharged by ownership instead. Nothing here needed an exception.

## Feature gating

This crate has no features and depends on `wgpu` unconditionally, because it
*is* the `wgpu` half -- a feature to remove `wgpu` from it would leave nothing.

There is no gating a level up either, and that is deliberate rather than an
omission. A `render` feature on `corvid_app` deciding whether this crate is
compiled at all is what would force a renderer's pipelines to be declared in a
crate that cannot name a device. What a build genuinely avoiding a graphics
stack looks like is a build of the simulation ring by name -- `cargo build -p corvid_behavior
-p corvid_replay -p corvid_lockstep --no-default-features` -- rather than a
feature switch on a workspace, which Cargo unifies anyway.
