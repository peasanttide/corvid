//! What each widget kind lays out as, and the two properties every layout has.
//!
//! The seam against `layout.rs` is the subject: that file is the box model --
//! lengths, gaps, padding, justification -- and this is the widgets, plus the
//! determinism and the visit count that hold for all of them.

#![allow(
    clippy::expect_used,
    clippy::panic_in_result_fn,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_fixed::{Factor16, I16F16};

use corvid_hash::digest;

use corvid_ui::{
    Element, Length, Monospace, Painted, Rect, Scale, TextStyle, TooLarge, Tree, button, column,
    label, paragraph, slider, solve, spacer, style, toggle,
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
