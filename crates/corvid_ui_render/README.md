# `corvid_ui_render`

The device half of a `corvid_ui` layout: two instanced pipelines, a scissor per
clip region, and the upload between them.

```rust
use corvid_fixed::I16F16;
use corvid_ui::{Monospace, Rect, Scale, Tree, column, label, solve};
use corvid_ui_render::{Grid, RectInstance, batches};

let mut tree = Tree::<()>::new();
tree.reconcile(column().child(label("score")));
let painted = solve(
    &tree,
    &Monospace::DEFAULT,
    Scale::DEFAULT,
    Rect::of(I16F16::from_f64(320.0), I16F16::from_f64(200.0)),
)?;

// Five glyphs under one scissor, and no rectangles because nothing was filled.
let batches = batches(&painted);
assert_eq!(batches.len(), 1);
assert_eq!(batches[0].glyphs, 0..5);
assert_eq!(RectInstance::LAYOUT.array_stride, 64);
# Ok::<(), corvid_ui::TooLarge>(())
```

## Where `wgpu` comes from

This crate's manifest does **not** name `wgpu`. It names `corvid_render`, which
owns the renderer, and the version is pinned once in the root manifest. One pin,
one version in the graph, one `raw-window-handle` that a surface and a window
can agree on. `tests/manifest.rs` reads this crate's own manifest and fails if a
`wgpu` line is ever added to it, which is the mechanical form of that rule.

## Why a rectangle is a distance field rather than nine slices

One instanced quad per rectangle, with the corner radius in the instance data,
draws a rounded and bordered rectangle in the fragment shader with no texture
and no geometry. A nine-slice needs an atlas entry per corner style and cannot
animate a radius. The cost is a `length()` per pixel of UI, which for a menu is
a rounding error and for a full-screen panel is under a hundred microseconds on
the machines this targets.

There is no vertex buffer at all: the quad is four vertices drawn as a triangle
strip and the corner comes from the vertex index, so a frame binds one instance
buffer per pipeline and nothing else.

## What a batch is

```rust
use corvid_ui_render::{Batch, batches};
use corvid_ui::Painted;

// Nothing to draw is no batches, which is no draw calls rather than an empty
// one.
assert!(batches(&Painted::default()).is_empty());
```

A clip region is a subtree and a subtree is contiguous in tree order, so one
run of a clip index is one run in the rectangle list and one in the glyph list
at once. Each batch draws its rectangles and then its glyphs, which is what
puts a label over the panel it sits on. A UI with one scroll region is two
batches; a UI with fifty is fifty, and that is the number to watch if a HUD ever
gets slow.

Within a single batch every rectangle is drawn before every glyph, so a panel
that overlaps another panel's label without clipping does not cover it. Give the
covering panel a `Style::clip` and it becomes its own batch, which does.

## The atlas

`Atlas` is where a glyph is in a coverage texture and how large it is on the
page. Rasterising a font is a different crate's job; this one needs the two
answers and nothing else.

```rust
use corvid_ui::GlyphId;
use corvid_ui_render::{Atlas as _, Grid};

// A bitmap font: sixteen by sixteen equal cells, starting at the space.
let atlas = Grid::new(16, 16, 32);
assert_eq!(atlas.uv(GlyphId(32)), [0.0, 0.0, 0.0625, 0.0625]);
// A glyph the atlas does not hold is a zero-area quad, which draws nothing.
assert_eq!(atlas.uv(GlyphId(0)), [0.0; 4]);
```

`Grid::quad` puts the cell three quarters above the baseline, which is where
`corvid_ui::Monospace` measures its ascent — so a layout and its glyphs agree
without a second table to keep in step.

## A layout need not be the size of its target

`Painter::draw` takes the attachment's size in physical pixels, and the vertex
stage divides by `Painted::size` — so a layout solved at one size is *stretched*
onto the target rather than cropped to it. That is what a game whose `View` is
never told how large its window is has to do, and it is what a fixed design
resolution is.

A scissor is in the target's pixels and a clip rectangle is in the layout's, so
the clips are carried across the same stretch. Without that a UI solved larger
than its window would scissor away everything past the window's own width, and
the symptom would be a scroll region that clipped correctly at one size and
vanished at another.

## Colour

A `Rgba8` in a style is sRGB, and what reaches the shader is
`Rgba8::to_linear()`. That is what an sRGB surface format wants: the device
encodes on store, so the value written is the linear one. A `Unorm` surface
without the `Srgb` suffix will look washed out, which is the same trade every
other technique in this workspace makes.
