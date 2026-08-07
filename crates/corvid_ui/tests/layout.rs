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
    Align, Axis, Edges, Element, Justify, Length, Monospace, Painted, Rect, Scale, Size, TextStyle,
    TooLarge, Tree, button, column, label, paragraph, row, slider, solve, spacer, style, toggle,
};
/// What the menus below raise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

#[test]
fn the_same_tree_solves_to_the_same_bytes() -> Result<(), TooLarge> {
    let build = || {
        column()
            .gap(px(4.0))
            .child(label("score"))
            .child(slider(Factor16::from_f64(0.25), Intent::Settings))
            .child(toggle(true, Intent::Friends))
    };
    let one = lay(build(), 320.0, 240.0)?;
    let two = lay(build(), 320.0, 240.0)?;
    assert_eq!(one, two);
    assert_eq!(digest(&one), digest(&two));
    Ok(())
}

#[test]
fn solving_visits_every_node_exactly_twice() -> Result<(), TooLarge> {
    let wide = column().children((0..4095).map(|_| spacer().width(px(1.0)).height(px(1.0))));
    let painted = lay(wide, 4096.0, 4096.0)?;
    assert_eq!(painted.visits.measured, 4096);
    assert_eq!(painted.visits.placed, 4096);
    assert_eq!(
        u32::from(painted.visits.measured == painted.visits.placed) * painted.visits.measured * 2,
        2 * 4096
    );
    Ok(())
}

#[test]
fn a_label_is_the_sum_of_its_advances() -> Result<(), TooLarge> {
    let painted = lay(column().child(label("hello")), 320.0, 240.0)?;
    // Five characters, eight pixels each; a line is five quarters of sixteen.
    assert_eq!(painted.nodes[1].rect.width, at(40.0));
    assert_eq!(painted.nodes[1].rect.height, at(20.0));
    assert_eq!(painted.glyphs.len(), 5);
    // The baseline is three quarters of the way down the line box.
    assert_eq!(painted.glyphs[0].at.y, at(15.0));
    assert_eq!(painted.glyphs[4].at.x, at(32.0));

    // Text at twice the size is twice as wide.
    let big = lay(
        column().child(label("hello").style(
            corvid_ui::Style::new().text(TextStyle::new(Length::rem(I16F16::from_f64(2.0)))),
        )),
        320.0,
        240.0,
    )?;
    assert_eq!(big.nodes[1].rect.width, at(80.0));
    Ok(())
}

#[test]
fn a_wrapped_label_breaks_in_the_same_places_everywhere() -> Result<(), TooLarge> {
    let painted = lay(
        column().child(paragraph("the quick brown fox jumps").width(px(80.0))),
        320.0,
        240.0,
    )?;

    // Ten characters to the line at eight pixels each: "the quick" is nine and
    // " brown" would be fifteen.
    assert_eq!(painted.nodes[1].rect.width, at(80.0));
    assert_eq!(painted.nodes[1].rect.height, at(60.0), "three lines of 20");
    assert_eq!(painted.glyphs.len(), 23, "the two breaking spaces are gone");

    let mut baselines: Vec<f64> = painted
        .glyphs
        .iter()
        .map(|glyph| glyph.at.y.to_f64())
        .collect();
    baselines.dedup();
    assert_eq!(baselines, vec![15.0, 35.0, 55.0]);
    Ok(())
}

#[test]
fn a_button_takes_the_focus_and_a_label_does_not() -> Result<(), TooLarge> {
    let painted = lay(
        column()
            .child(label("cradle"))
            .child(button("play", Intent::Play)),
        320.0,
        240.0,
    )?;
    let focusable: Vec<bool> = painted.nodes.iter().map(|node| node.focusable).collect();
    assert_eq!(
        focusable,
        vec![false, false, true, false],
        "the column, the label, the button, and the button's own label"
    );
    Ok(())
}

#[test]
fn a_slider_is_a_factor_and_a_step_of_zero_cannot_be_nudged() -> Result<(), TooLarge> {
    let painted = lay(
        column().child(slider(Factor16::from_f64(0.25), Intent::Settings)),
        320.0,
        240.0,
    )?;
    // Eight rems wide, a quarter of it filled. A `Factor16` is a UNORM, so a
    // quarter is 16384/65535 and lands half a thousandth of a pixel past 32.
    assert_eq!(painted.nodes[1].rect.width, at(128.0));
    let filled = painted.rects.last().unwrap().rect.width.to_f64();
    assert!((filled - 32.0).abs() < 0.001, "filled {filled}");

    let mut tree = Tree::new();
    tree.reconcile(
        column().child(slider(Factor16::from_f64(0.5), Intent::Settings).step(Factor16::ZERO)),
    );
    let painted = solve(
        &tree,
        &Monospace::DEFAULT,
        Scale::DEFAULT,
        Rect::of(at(320.0), at(240.0)),
    )?;
    let handle = painted.nodes[1].node;
    tree.focus_on(handle);
    let mut raised = Vec::new();
    tree.nudge(true, &mut raised);
    assert!(raised.is_empty(), "a step of zero moves nothing");

    tree.drag(Factor16::MAX, &mut raised);
    assert_eq!(raised.len(), 1, "and a drag still moves it");
    Ok(())
}

#[test]
fn the_menu_from_the_specification_lays_out() -> Result<(), TooLarge> {
    let menu = column()
        .gap(Length::rem(I16F16::ONE))
        .child(label("cradle").style(style::TITLE))
        .child(button("play", Intent::Play))
        .child(button("join a friend", Intent::Friends))
        .child(button("settings", Intent::Settings));

    let painted = lay(menu, 1280.0, 720.0)?;
    assert_eq!(painted.focusable().count(), 3);
    // Two rems of title, and a rem of air under it.
    assert_eq!(painted.nodes[1].rect.height, at(40.0));
    assert_eq!(painted.nodes[2].rect.y, at(72.0));
    // Frozen: the menu in the specification, resolved.
    assert_eq!(digest(&painted).to_u64(), 12_982_146_453_585_033_227);
    Ok(())
}

#[test]
fn an_empty_tree_solves_to_nothing() -> Result<(), TooLarge> {
    let tree = Tree::<Intent>::new();
    let painted = solve(
        &tree,
        &Monospace::DEFAULT,
        Scale::DEFAULT,
        Rect::of(at(320.0), at(240.0)),
    )?;
    assert!(painted.nodes.is_empty());
    assert_eq!(painted.size, Rect::of(at(320.0), at(240.0)));
    Ok(())
}

#[test]
fn a_clipped_node_gives_its_children_a_scissor() -> Result<(), TooLarge> {
    let painted = lay(
        column()
            .width(px(50.0))
            .height(px(50.0))
            .style(
                corvid_ui::Style::new()
                    .clip(true)
                    .width(px(50.0))
                    .height(px(50.0)),
            )
            .child(spacer().width(px(200.0)).height(px(200.0))),
        320.0,
        240.0,
    )?;
    assert_eq!(painted.clips.len(), 2);
    assert_eq!(painted.clips[1], Rect::of(at(50.0), at(50.0)));
    assert_eq!(painted.nodes[0].clip, 0);
    assert_eq!(painted.nodes[1].clip, 1);
    Ok(())
}
