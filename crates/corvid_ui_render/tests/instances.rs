//! What a layout becomes on the way to a device, checked without one.
//!
//! Every assertion here is about the conversion and the batching, which are
//! the two things in this crate that can be wrong in a way a picture would not
//! show. What a driver does with the result is `corvid_render`'s offscreen
//! tests' subject, and needs an adapter.

#![allow(
    clippy::expect_used,
    clippy::panic_in_result_fn,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::float_cmp,
    reason = "every float here is the exact conversion of a fixed-point value or an exact binary fraction of a power-of-two grid, so a strict comparison is what the assertion means; a tolerance would let a real drift through"
)]

use corvid_color::Rgba8;
use corvid_fixed::I16F16;
use corvid_render::Extent;
use corvid_ui::{
    Edges, GlyphId, Length, Monospace, Painted, PaintedGlyph, PaintedRect, Position, Rect, Scale,
    Style, Tree, column, label, solve, spacer, style,
};
use corvid_ui_render::{Atlas as _, GlyphInstance, Grid, RectInstance, batches, scissor};

/// A number, spelled the way the assertions read.
const fn at(pixels: f64) -> I16F16 {
    I16F16::from_f64(pixels)
}

/// The atlas every test here draws through.
const ATLAS: Grid = Grid::new(16, 16, 32);

/// A menu with a panel, a label and a scroll region, so every path here has
/// something to look at.
fn painted() -> Result<Painted, corvid_ui::TooLarge> {
    let mut tree = Tree::<()>::new();
    tree.reconcile(
        column()
            .style(style::PANEL)
            .child(label("score"))
            .child(
                column()
                    .style(
                        Style::new()
                            .clip(true)
                            .width(Length::px(at(64.0)))
                            .height(Length::px(at(32.0)))
                            .padding(Edges::NONE)
                            .background(Rgba8::hex(0x11_22_33_FF)),
                    )
                    .child(label("scrolled")),
            )
            .child(spacer()),
    );
    solve(
        &tree,
        &Monospace::DEFAULT,
        Scale::DEFAULT,
        Rect::of(at(320.0), at(240.0)),
    )
}

#[test]
fn an_instance_is_the_bytes_a_pipeline_reads() {
    assert_eq!(size_of::<RectInstance>(), 64);
    assert_eq!(RectInstance::LAYOUT.array_stride, 64);
    assert_eq!(size_of::<GlyphInstance>(), 48);
    assert_eq!(GlyphInstance::LAYOUT.array_stride, 48);

    for layout in [RectInstance::LAYOUT, GlyphInstance::LAYOUT] {
        assert_eq!(layout.step_mode, wgpu::VertexStepMode::Instance);
        let last = layout.attributes.last().unwrap();
        assert!(
            last.offset + last.format.size() <= layout.array_stride,
            "the attributes have to fit in the stride they are read at"
        );
    }
}

#[test]
fn a_rectangle_becomes_its_four_vectors() {
    let painted = PaintedRect {
        rect: Rect::new(at(1.0), at(2.0), at(3.0), at(4.0)),
        fill: Rgba8::WHITE,
        border: Rgba8::BLACK,
        border_width: at(2.0),
        corner: at(8.0),
        clip: 3,
    };
    let instance = RectInstance::from(&painted);
    assert_eq!(instance.rect, [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(instance.fill, [1.0, 1.0, 1.0, 1.0], "white is one, linear");
    assert_eq!(instance.border, [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(instance.params, [2.0, 8.0, 0.0, 0.0]);
    // Every field is bytes with no padding between them, which is what makes a
    // slice of these an instance buffer with no `unsafe`.
    assert_eq!(bytemuck::bytes_of(&instance).len(), 64);
}

#[test]
fn a_glyph_sits_on_its_baseline() {
    let painted = PaintedGlyph {
        at: Position::new(at(10.0), at(100.0)),
        glyph: GlyphId(33),
        size: at(16.0),
        tint: Rgba8::WHITE,
        clip: 0,
    };
    let instance = GlyphInstance::new(&painted, &ATLAS);
    // Three quarters of the em above the baseline, and one em square.
    assert_eq!(instance.rect, [10.0, 88.0, 16.0, 16.0]);
    assert_eq!(instance.uv, ATLAS.uv(GlyphId(33)));
    assert_eq!(instance.uv, [0.0625, 0.0, 0.125, 0.0625]);
}

#[test]
fn a_grid_refuses_a_glyph_it_does_not_hold() {
    assert_eq!(ATLAS.uv(GlyphId(31)), [0.0; 4], "before the first cell");
    assert_eq!(ATLAS.uv(GlyphId(32 + 256)), [0.0; 4], "past the last");
    assert_eq!(Grid::new(0, 0, 0).uv(GlyphId(0)), [0.0; 4], "no cells");
}

#[test]
fn nothing_to_draw_is_no_draw_calls() {
    assert!(batches(&Painted::default()).is_empty());
}

#[test]
fn a_clip_region_is_its_own_batch() -> Result<(), corvid_ui::TooLarge> {
    let painted = painted()?;
    assert_eq!(
        painted.clips.len(),
        2,
        "the viewport, and the scroll region"
    );

    let batches = batches(&painted);
    assert!(
        batches.len() >= 2,
        "the scroll region splits the draw: {batches:?}"
    );
    assert!(
        batches.iter().any(|batch| batch.clip == 1),
        "and one of them is scissored to it"
    );

    // Every instance is in exactly one batch, and the batches are in order.
    let rects: usize = batches.iter().map(|batch| batch.rects.len()).sum();
    let glyphs: usize = batches.iter().map(|batch| batch.glyphs.len()).sum();
    assert_eq!(rects, painted.rects.len());
    assert_eq!(glyphs, painted.glyphs.len());
    for pair in batches.windows(2) {
        assert_eq!(pair[0].rects.end, pair[1].rects.start);
        assert_eq!(pair[0].glyphs.end, pair[1].glyphs.start);
    }
    Ok(())
}

#[test]
fn a_scissor_is_whole_pixels_inside_the_target() {
    let viewport = Extent::new(320, 240);
    assert_eq!(
        scissor(Rect::new(at(10.5), at(20.5), at(30.0), at(40.0)), viewport),
        Some((10, 20, 30, 40)),
        "rounded down, because a scissor is whole pixels"
    );
    assert_eq!(
        scissor(Rect::new(at(300.0), at(0.0), at(100.0), at(10.0)), viewport),
        Some((300, 0, 20, 10)),
        "and cut to the target rather than overrunning it"
    );
    assert_eq!(
        scissor(Rect::new(at(400.0), at(0.0), at(10.0), at(10.0)), viewport),
        None,
        "a region entirely off the target is no draw rather than a validation error"
    );
    assert_eq!(scissor(Rect::ZERO, viewport), None);
}
