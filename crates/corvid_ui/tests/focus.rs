//! One tree, four devices, one node.
//!
//! This file is the crate's reason for existing. A gamepad, a mouse, a touch
//! screen and an XR ray are four different pieces of hardware and two
//! primitives: a step in a direction, and a point in layout space. Everything
//! here is a check that the second primitive really is the same one for three
//! of the four, and that the first is spatial in the way a stick expects.

#![allow(
    clippy::expect_used,
    clippy::panic_in_result_fn,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_fixed::I24F8;

use corvid_fixed::I16F16;
use corvid_shape::Ray;
use corvid_transform::Transform;
use corvid_ui::{
    Compass, Edges, Element, Focus, Length, Monospace, NodeId, Painted, Position, Rect, Scale,
    Signal, TooLarge, Tree, button, column, label, row, solve, spacer,
};
use corvid_vector::{Direction, GlobalPoint};
/// What the menu below raises.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Intent {
    Play,
    Settings,
}

/// Pixels.
const fn px(pixels: f64) -> Length {
    Length::px(I16F16::from_f64(pixels))
}

/// A number.
const fn at(pixels: f64) -> I16F16 {
    I16F16::from_f64(pixels)
}

/// The viewport every test here lays out in.
const WIDTH: f64 = 640.0;
/// The other half of it.
const HEIGHT: f64 = 480.0;

/// Lay a tree out, keeping the tree so the focus can be driven through it.
fn lay(element: Element<Intent>) -> Result<(Tree<Intent>, Painted), TooLarge> {
    let mut tree = Tree::new();
    tree.reconcile(element);
    let painted = solve(
        &tree,
        &Monospace::DEFAULT,
        Scale::DEFAULT,
        Rect::of(at(WIDTH), at(HEIGHT)),
    )?;
    Ok((tree, painted))
}

/// Two buttons, one above the other.
fn menu() -> Element<Intent> {
    column()
        .gap(px(8.0))
        .child(button("play", Intent::Play))
        .child(button("settings", Intent::Settings))
}

/// The ray that lands on a layout position, cast at a panel a metre across.
///
/// The panel is [`Transform::IDENTITY`]: at the origin, facing `+Y`, with `+Z`
/// up and `+X` right. So a ray starting two metres behind it and travelling
/// along `+Y` lands where its `x` and `z` say it does, which is what makes the
/// arithmetic here readable rather than a matrix.
fn ray_at(target: Position, metres: f64) -> Ray {
    let half_width = metres / 2.0;
    let half_height = half_width * HEIGHT / WIDTH;
    let across = (target.x.to_f64() / WIDTH).mul_add(metres, -half_width);
    let up = (target.y.to_f64() / HEIGHT).mul_add(-(2.0 * half_height), half_height);
    Ray::new(
        GlobalPoint::new(
            I24F8::from_f64(across),
            I24F8::from_f64(-2.0),
            I24F8::from_f64(up),
        ),
        Direction::Y,
    )
}

/// **The deliverable.** Four devices, one tree, one node.
#[test]
fn a_pad_a_mouse_a_finger_and_a_ray_all_reach_the_same_node() -> Result<(), TooLarge> {
    let (mut tree, painted) = lay(menu())?;
    let play = painted.nodes[1].node;
    let settings = painted.nodes[3].node;
    let target = painted.rect_of(settings).unwrap();
    assert_ne!(play, settings);

    // A gamepad: the focus is on the button above, and the stick goes down.
    tree.point(&painted, painted.rect_of(play).unwrap().centre());
    assert_eq!(tree.navigate(&painted, Compass::Down).node, settings);

    // A mouse: a position over the button.
    let mut mouse = Tree::new();
    mouse.reconcile(menu());
    assert_eq!(mouse.point(&painted, target.centre()).node, settings);

    // A touch screen: a fingertip is a point too, and it is nowhere near the
    // centre of anything.
    let finger = Position::new(
        target.x.saturating_add(at(3.0)),
        target.y.saturating_add(at(2.0)),
    );
    let mut touch = Tree::new();
    touch.reconcile(menu());
    assert_eq!(touch.point(&painted, finger).node, settings);

    // An XR ray: cast at the panel, then the same call the mouse made.
    let landed = painted
        .panel_to_layout(
            Transform::IDENTITY,
            I16F16::ONE,
            ray_at(target.centre(), 1.0),
        )
        .expect("the ray was aimed at the panel");
    let mut headset = Tree::new();
    headset.reconcile(menu());
    assert_eq!(headset.point(&painted, landed).node, settings);

    // And the thing that holds the focus answers the same, however it arrived.
    let mut raised = Vec::new();
    headset.signal(Signal::Activate, &mut raised);
    assert_eq!(raised.len(), 1);
    assert_eq!(raised[0].intent, Intent::Settings);
    assert_eq!(raised[0].node, settings);
    Ok(())
}

/// Two columns of different heights, arranged so that an unweighted score
/// picks the wrong column and the `2 * across` weighting picks the right one.
fn grid() -> Element<Intent> {
    row()
        .child(
            column()
                .child(spacer().width(px(50.0)).height(px(100.0)).focusable(true))
                .child(spacer().width(px(50.0)).height(px(100.0)).focusable(true)),
        )
        .child(
            column()
                .margin(Edges::new(px(60.0), Length::ZERO, Length::ZERO, px(50.0)))
                .child(spacer().width(px(100.0)).height(px(20.0)).focusable(true))
                .child(spacer().width(px(100.0)).height(px(20.0)).focusable(true)),
        )
}

#[test]
fn a_stick_down_stays_in_its_own_column() -> Result<(), TooLarge> {
    let (mut tree, painted) = lay(grid())?;
    let focusable: Vec<NodeId> = painted.focusable().map(|node| node.node).collect();
    let (top_left, below, top_right) = (focusable[0], focusable[1], focusable[2]);

    // The geometry the weighting is about, stated rather than assumed: `along`
    // is the distance between the centres downwards, and `across` is the gap
    // between the two rectangles sideways.
    let from = painted.rect_of(top_left).unwrap();
    let score = |to: Rect| {
        let along = to.centre().y.to_f64() - from.centre().y.to_f64();
        let across = (from.x.to_f64() - to.right().to_f64())
            .max(to.x.to_f64() - from.right().to_f64())
            .max(0.0);
        (along, across)
    };
    let (down_along, down_across) = score(painted.rect_of(below).unwrap());
    let (side_along, side_across) = score(painted.rect_of(top_right).unwrap());
    assert!(
        down_across <= 0.0,
        "a column's rows line up, so their gap is nothing"
    );
    assert!(
        side_along + side_across < down_along + down_across,
        "unweighted, the other column is nearer"
    );
    assert!(
        2.0f64.mul_add(side_across, side_along) > 2.0f64.mul_add(down_across, down_along),
        "weighted, it is not"
    );

    tree.focus_on(top_left);
    assert_eq!(tree.navigate(&painted, Compass::Down).node, below);
    Ok(())
}

/// A column whose buttons are different widths, which is every real menu:
/// their edges line up and their centres do not.
#[test]
fn a_stick_down_a_ragged_column_takes_every_row_in_turn() -> Result<(), TooLarge> {
    let (mut tree, painted) = lay(column()
        .gap(px(16.0))
        .child(button("play", Intent::Play))
        .child(button("join a friend", Intent::Settings))
        .child(button("settings", Intent::Play))
        .child(button("quit", Intent::Settings)))?;
    let rows: Vec<NodeId> = painted.focusable().map(|node| node.node).collect();

    tree.focus_on(rows[0]);
    let mut walked = alloc_walk(&mut tree, &painted, rows.len() - 1);
    walked.insert(0, rows[0]);
    assert_eq!(
        walked, rows,
        "scored between centres, the widest row's neighbour is the row after next"
    );
    Ok(())
}

/// The focus, stepped down this many times.
fn alloc_walk(tree: &mut Tree<Intent>, painted: &Painted, steps: usize) -> Vec<NodeId> {
    (0..steps)
        .map(|_| tree.navigate(painted, Compass::Down).node)
        .collect()
}

#[test]
fn a_step_off_the_edge_stays_where_it_was() -> Result<(), TooLarge> {
    let (mut tree, painted) = lay(menu())?;
    let play = painted.nodes[1].node;
    tree.focus_on(play);
    assert_eq!(tree.navigate(&painted, Compass::Up).node, play);
    assert_eq!(tree.navigate(&painted, Compass::Left).node, play);
    Ok(())
}

#[test]
fn tab_wraps_and_the_compass_does_not() -> Result<(), TooLarge> {
    let (mut tree, painted) = lay(menu())?;
    let focusable: Vec<NodeId> = painted.focusable().map(|node| node.node).collect();

    // With nothing focused, the first step lands on the first one.
    assert_eq!(tree.navigate(&painted, Compass::Next).node, focusable[0]);
    assert_eq!(tree.navigate(&painted, Compass::Next).node, focusable[1]);
    assert_eq!(
        tree.navigate(&painted, Compass::Next).node,
        focusable[0],
        "the end wraps to the start"
    );
    assert_eq!(
        tree.navigate(&painted, Compass::Previous).node,
        focusable[1],
        "and backwards from the start wraps to the end"
    );
    // Down, from the last one, has nowhere to go.
    tree.focus_on(focusable[1]);
    assert_eq!(tree.navigate(&painted, Compass::Down).node, focusable[1]);
    Ok(())
}

#[test]
fn the_focus_survives_a_sibling_changing() -> Result<(), TooLarge> {
    let build = |score: &str| {
        column()
            .child(label(score))
            .child(button("settings", Intent::Settings))
    };
    let mut tree = Tree::new();
    tree.reconcile(build("nil"));
    let painted = solve(
        &tree,
        &Monospace::DEFAULT,
        Scale::DEFAULT,
        Rect::of(at(WIDTH), at(HEIGHT)),
    )?;
    let settings = painted.focusable().next().unwrap().node;
    tree.focus_on(settings);

    tree.reconcile(build("one"));
    assert_eq!(tree.focus().node, settings);
    assert!(tree.focus().visible);
    Ok(())
}

/// Rows that say who they are, so a moved row is visible in the assertion.
fn rows(order: &[u64], keyed: bool) -> Element<Intent> {
    column().children(order.iter().map(|id| {
        let row = spacer()
            .width(px(100.0))
            .height(px(f64::from(u32::try_from(*id).unwrap_or(1)) * 10.0))
            .focusable(true);
        if keyed { row.keyed(*id) } else { row }
    }))
}

#[test]
fn a_named_row_takes_its_focus_with_it_and_a_positional_one_does_not() -> Result<(), TooLarge> {
    let (mut tree, painted) = lay(rows(&[1, 2, 3], true))?;
    let third = painted.focusable().nth(2).unwrap().node;
    tree.focus_on(third);
    tree.reconcile(rows(&[3, 1, 2], true));
    assert_eq!(
        tree.focus().node,
        third,
        "a named row is the same node wherever it moved to"
    );
    let height = tree.node(third).map(|node| node.style.height);
    assert_eq!(height, Some(px(30.0)), "and it is still the third row");

    let (mut tree, painted) = lay(rows(&[1, 2, 3], false))?;
    let third = painted.focusable().nth(2).unwrap().node;
    tree.focus_on(third);
    tree.reconcile(rows(&[3, 1, 2], false));
    assert_eq!(
        tree.focus().node,
        third,
        "a positional row's focus stays on the position"
    );
    let height = tree.node(third).map(|node| node.style.height);
    assert_eq!(
        height,
        Some(px(20.0)),
        "which is now a different row: the third position holds what was second"
    );
    Ok(())
}

#[test]
fn a_point_over_nothing_blurs() -> Result<(), TooLarge> {
    let (mut tree, painted) = lay(menu())?;
    let settings = painted.nodes[3].node;
    tree.point(&painted, painted.rect_of(settings).unwrap().centre());
    assert_eq!(tree.focus().node, settings);

    let nowhere = Position::new(at(WIDTH - 1.0), at(HEIGHT - 1.0));
    assert_eq!(tree.point(&painted, nowhere), Focus::NOWHERE);

    // And a signal at nothing raises nothing rather than firing at a stale
    // focus.
    let mut raised = Vec::new();
    tree.signal(Signal::Activate, &mut raised);
    assert!(raised.is_empty());
    Ok(())
}

#[test]
fn a_ray_that_misses_the_quad_is_nothing() -> Result<(), TooLarge> {
    let (_, painted) = lay(menu())?;
    let centre = Position::new(at(WIDTH / 2.0), at(HEIGHT / 2.0));

    // Aimed at the middle, it lands in the middle.
    let landed = painted
        .panel_to_layout(Transform::IDENTITY, I16F16::ONE, ray_at(centre, 1.0))
        .unwrap();
    assert!((landed.x.to_f64() - centre.x.to_f64()).abs() < 4.0);
    assert!((landed.y.to_f64() - centre.y.to_f64()).abs() < 4.0);

    // Ten metres to the side of a panel one metre across.
    let wide = Ray::new(
        GlobalPoint::new(I24F8::from_f64(10.0), I24F8::from_f64(-2.0), I24F8::ZERO),
        Direction::Y,
    );
    assert_eq!(
        painted.panel_to_layout(Transform::IDENTITY, I16F16::ONE, wide),
        None
    );

    // And a ray pointing away from it never arrives.
    let away = Ray::new(
        GlobalPoint::new(I24F8::ZERO, I24F8::from_f64(-2.0), I24F8::ZERO),
        -Direction::Y,
    );
    assert_eq!(
        painted.panel_to_layout(Transform::IDENTITY, I16F16::ONE, away),
        None
    );

    // A panel of no width is not a panel.
    assert_eq!(
        painted.panel_to_layout(Transform::IDENTITY, I16F16::ZERO, ray_at(centre, 1.0)),
        None
    );
    Ok(())
}

#[test]
fn a_toggle_flips_and_says_it_changed() -> Result<(), TooLarge> {
    let (mut tree, painted) = lay(column().child(corvid_ui::toggle(false, Intent::Play)))?;
    let handle = painted.focusable().next().unwrap().node;
    tree.focus_on(handle);

    let mut raised = Vec::new();
    tree.signal(Signal::Activate, &mut raised);
    assert_eq!(raised.len(), 1);
    assert_eq!(raised[0].signal, Signal::Change);
    assert_eq!(
        tree.node(handle).map(|node| node.kind),
        Some(corvid_ui::Kind::Toggle {
            on: true,
            intent: Intent::Play
        })
    );
    Ok(())
}
