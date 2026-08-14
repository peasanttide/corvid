//! The flex solver, case by case, with the arithmetic written out.
//!
//! Every expected number here is one a person can do on paper: sixteen pixels
//! to the rem, half of that to a character, five quarters of it to a line. A
//! test that says `40.0` and means "five characters at eight pixels" is a test
//! that fails loudly when the solver drifts, which a tolerance would not be.

#![allow(
    clippy::expect_used,
    clippy::panic_in_result_fn,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_fixed::{Factor16, I16F16};

use corvid_hash::digest;

use corvid_ui::{
    Align, Axis, Edges, Element, Justify, Length, Monospace, Painted, Rect, Scale, Size, TooLarge,
    Tree, column, row, solve, spacer, style,
};
/// What the menus below raise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[expect(
    dead_code,
    reason = "the layout here is built from the same widget vocabulary the other test binaries use, and each of them names a different part of it"
)]
enum Intent {
    Play,
    Friends,
    Settings,
}

/// Pixels, spelled the way the assertions read.
const fn px(pixels: f64) -> Length {
    Length::px(I16F16::from_f64(pixels))
}

/// A number, spelled the way the assertions read.
const fn at(pixels: f64) -> I16F16 {
    I16F16::from_f64(pixels)
}

/// Lay a tree out in a viewport of this size.
fn lay(element: Element<Intent>, width: f64, height: f64) -> Result<Painted, TooLarge> {
    let mut tree = Tree::new();
    tree.reconcile(element);
    solve(
        &tree,
        &Monospace::DEFAULT,
        Scale::DEFAULT,
        Rect::of(at(width), at(height)),
    )
}

/// Every node's rectangle, in tree order.
fn rects(painted: &Painted) -> Vec<(f64, f64, f64, f64)> {
    painted
        .nodes
        .iter()
        .map(|node| {
            (
                node.rect.x.to_f64(),
                node.rect.y.to_f64(),
                node.rect.width.to_f64(),
                node.rect.height.to_f64(),
            )
        })
        .collect()
}

#[test]
fn a_length_is_exact_at_both_ends() {
    let scale = Scale::DEFAULT;
    let hundred = at(100.0);
    assert_eq!(
        Length::Fraction(Factor16::ZERO).resolve(scale, hundred, I16F16::ZERO),
        I16F16::ZERO
    );
    assert_eq!(Length::FULL.resolve(scale, hundred, I16F16::ZERO), hundred);
    assert_eq!(Length::REM.resolve(scale, hundred, I16F16::ZERO), at(16.0));
    assert_eq!(px(3.5).resolve(scale, hundred, I16F16::ZERO), at(3.5));
    assert_eq!(
        Length::Auto.resolve(scale, hundred, at(7.0)),
        at(7.0),
        "auto is as large as the content the caller measured"
    );

    // A rem follows the display it was built for.
    assert_eq!(Scale::DEFAULT.rem, at(16.0));
    assert_eq!(Scale::for_dpi(at(192.0)).rem, at(32.0));
}

#[test]
fn a_column_places_its_children_at_the_offsets_arithmetic_gives() -> Result<(), TooLarge> {
    let painted = lay(
        column()
            .gap(px(8.0))
            .child(spacer().width(px(20.0)).height(px(10.0)))
            .child(spacer().width(px(20.0)).height(px(20.0)))
            .child(spacer().width(px(20.0)).height(px(30.0))),
        200.0,
        200.0,
    )?;

    assert_eq!(
        rects(&painted),
        vec![
            // Auto in both directions: 20 wide, and 10 + 8 + 20 + 8 + 30 tall.
            (0.0, 0.0, 20.0, 76.0),
            (0.0, 0.0, 20.0, 10.0),
            (0.0, 18.0, 20.0, 20.0),
            (0.0, 46.0, 20.0, 30.0),
        ]
    );
    // Frozen: a layout regression is a changed digest rather than a screenshot.
    assert_eq!(digest(&painted).to_u64(), 4_286_894_713_342_283_318);
    Ok(())
}

#[test]
fn three_thirds_fill_exactly_a_hundred() -> Result<(), TooLarge> {
    let third = Length::Fraction(Factor16::from_f64(1.0 / 3.0));
    let painted = lay(
        row()
            .width(px(100.0))
            .height(px(10.0))
            .child(spacer().width(third))
            .child(spacer().width(third))
            .child(spacer().width(third)),
        200.0,
        200.0,
    )?;

    let widths: Vec<I16F16> = painted.nodes[1..]
        .iter()
        .map(|node| node.rect.width)
        .collect();
    let total = widths
        .iter()
        .fold(I16F16::ZERO, |sum, width| sum.saturating_add(*width));
    assert_eq!(total, at(100.0), "the row is exactly full: {widths:?}");
    assert_eq!(
        painted.nodes[3].rect.right(),
        at(100.0),
        "and the remainder went to the last of them"
    );
    Ok(())
}

#[test]
fn justify_puts_the_leftover_where_it_says() -> Result<(), TooLarge> {
    // Two ten-pixel children in a hundred pixels: eighty over.
    let build = |justify: Justify, axis: Axis| {
        column()
            .axis(axis)
            .justify(justify)
            .width(px(100.0))
            .height(px(100.0))
            .child(spacer().width(px(10.0)).height(px(10.0)))
            .child(spacer().width(px(10.0)).height(px(10.0)))
    };
    let starts = |painted: &Painted, axis: Axis| -> Vec<f64> {
        painted.nodes[1..]
            .iter()
            .map(|node| {
                if axis.is_horizontal() {
                    node.rect.x.to_f64()
                } else {
                    node.rect.y.to_f64()
                }
            })
            .collect()
    };

    for axis in [Axis::Row, Axis::Column] {
        assert_eq!(
            starts(&lay(build(Justify::Start, axis), 200.0, 200.0)?, axis),
            vec![0.0, 10.0]
        );
        assert_eq!(
            starts(&lay(build(Justify::Centre, axis), 200.0, 200.0)?, axis),
            vec![40.0, 50.0]
        );
        assert_eq!(
            starts(&lay(build(Justify::End, axis), 200.0, 200.0)?, axis),
            vec![80.0, 90.0]
        );
        assert_eq!(
            starts(&lay(build(Justify::Between, axis), 200.0, 200.0)?, axis),
            vec![0.0, 90.0]
        );
        assert_eq!(
            starts(&lay(build(Justify::Around, axis), 200.0, 200.0)?, axis),
            vec![20.0, 70.0]
        );
    }
    Ok(())
}

#[test]
fn align_puts_the_child_across_the_axis_where_it_says() -> Result<(), TooLarge> {
    let build = |align: Align, axis: Axis| {
        column()
            .axis(axis)
            .align(align)
            .width(px(100.0))
            .height(px(100.0))
            .child(spacer().width(px(10.0)).height(px(10.0)))
    };
    let across = |painted: &Painted, axis: Axis| -> (f64, f64) {
        let rect = painted.nodes[1].rect;
        if axis.is_horizontal() {
            (rect.y.to_f64(), rect.height.to_f64())
        } else {
            (rect.x.to_f64(), rect.width.to_f64())
        }
    };

    for axis in [Axis::Row, Axis::Column] {
        assert_eq!(
            across(&lay(build(Align::Start, axis), 200.0, 200.0)?, axis),
            (0.0, 10.0)
        );
        assert_eq!(
            across(&lay(build(Align::Centre, axis), 200.0, 200.0)?, axis),
            (45.0, 10.0)
        );
        assert_eq!(
            across(&lay(build(Align::End, axis), 200.0, 200.0)?, axis),
            (90.0, 10.0)
        );
        // Stretch overrides a size the child did not ask for; here it asked.
        assert_eq!(
            across(&lay(build(Align::Stretch, axis), 200.0, 200.0)?, axis),
            (0.0, 10.0)
        );
    }

    // And with no size of its own, stretch is the whole width.
    let stretched = lay(
        column()
            .align(Align::Stretch)
            .width(px(100.0))
            .child(spacer().height(px(10.0))),
        200.0,
        200.0,
    )?;
    assert_eq!(stretched.nodes[1].rect.width, at(100.0));
    Ok(())
}

#[test]
fn padding_shrinks_the_content_and_margin_shrinks_the_box() -> Result<(), TooLarge> {
    let padded = lay(
        column()
            .padding(Edges::all(px(5.0)))
            .child(spacer().width(px(10.0)).height(px(10.0))),
        200.0,
        200.0,
    )?;
    assert_eq!(
        rects(&padded),
        vec![
            // The padding is inside the parent, which grew to hold it.
            (0.0, 0.0, 20.0, 20.0),
            (5.0, 5.0, 10.0, 10.0),
        ]
    );

    let margined = lay(
        column().child(
            spacer()
                .width(px(10.0))
                .height(px(10.0))
                .margin(Edges::all(px(5.0))),
        ),
        200.0,
        200.0,
    )?;
    assert_eq!(
        rects(&margined),
        vec![
            // The margin is outside the child and inside the parent, which is
            // the same twenty pixels reached the other way round.
            (0.0, 0.0, 20.0, 20.0),
            (5.0, 5.0, 10.0, 10.0),
        ]
    );
    Ok(())
}

#[test]
fn a_clamped_child_returns_its_space_to_its_siblings() -> Result<(), TooLarge> {
    let half = Length::Fraction(Factor16::from_f64(0.5));
    let painted = lay(
        row()
            .width(px(100.0))
            .height(px(10.0))
            .child(
                spacer().width(half).style(
                    style::PANEL
                        .width(half)
                        .max(Size::new(px(20.0), Length::Auto))
                        .padding(Edges::NONE),
                ),
            )
            .child(spacer().width(half)),
        200.0,
        200.0,
    )?;

    assert_eq!(painted.nodes[1].rect.width, at(20.0), "cut down to its max");
    assert_eq!(
        painted.nodes[2].rect.width,
        at(80.0),
        "and the thirty it gave back went to its sibling"
    );
    Ok(())
}

#[test]
fn a_width_past_the_type_names_the_node_rather_than_saturating() {
    let error = lay(
        row()
            .padding(Edges::all(px(1.0)))
            .child(spacer().width(Length::px(I16F16::MAX))),
        200.0,
        200.0,
    )
    .expect_err("a child as wide as I16F16 holds cannot also fit a padding");
    assert_eq!(error.axis, Axis::Row);
    assert_eq!(
        error.node.0, 1,
        "the node named is the one whose arithmetic ran past the type"
    );
    assert!(!format!("{error}").is_empty());
}
