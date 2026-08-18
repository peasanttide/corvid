//! A walk that arrives from a starting guess the grid got wrong.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

#[allow(
    dead_code,
    unreachable_pub,
    reason = "one fixture module serves every test file, and each file uses the surfaces it needs"
)]
mod surface;

use corvid_nav::NavTriRef;

use surface::{apart, fold, metres, world};

/// The whole valley is inside one 32-metre cell, so the grid has one bucket
/// for four triangles and the first of them is wrong for three.
///
/// That is the fixture's point: what is being tested is that the answer does
/// not depend on the guess.
#[test]
fn the_grid_has_one_cell_for_the_whole_fold() {
    let mesh = fold();
    assert_eq!(mesh.len(), 4);
    let grid = mesh.grid();
    let corner = grid.cell_of(metres(0.0, 0.0, 2.0));
    assert_eq!(grid.tris_in(corner).count(), 4, "one cell, four triangles");
    assert_eq!(grid.len(), 4, "and no other cell holds anything");
}

/// A walk that starts in the far triangle of the fold arrives in the near one.
///
/// The target is on the descending panel at `(0.7, 0.7)`, where the surface has
/// fallen 0.467 m of its two metres. Starting from triangle 3 -- the far corner
/// of the ascending panel, two seams away -- the walk crosses the valley floor
/// and lands on the triangle that actually holds the point.
#[test]
fn a_walk_from_the_wrong_triangle_still_arrives() {
    let mesh = fold();
    let target = metres(0.7, 0.7, 2.0 - 0.7 * 2.0 / 3.0);

    let arrived = mesh
        .walk_toward(NavTriRef(3), target)
        .expect("a walk on a mesh that has triangles");
    assert_eq!(arrived, NavTriRef(0));

    // And the coordinates it hands back are the target, not merely the right
    // triangle: a walk that stopped one seam short would still name a triangle.
    let cords = mesh.locate(target).expect("somewhere on the surface");
    assert_eq!(cords.tri, NavTriRef(0));
    let there = world(&mesh, cords.tri, cords.decode().position);
    assert!(
        apart(there, target) < 0.05,
        "landed at {there} rather than {target}"
    );
}

/// Every triangle of the fold can be reached from every other, whichever guess
/// the walk starts from.
#[test]
fn every_start_reaches_every_target() {
    let mesh = fold();
    let targets = [
        (NavTriRef(0), metres(0.7, 0.7, 1.533)),
        (NavTriRef(1), metres(2.3, 2.3, 0.467)),
        (NavTriRef(2), metres(0.7, 3.7, 0.467)),
        (NavTriRef(3), metres(2.3, 5.3, 1.533)),
    ];
    for (expected, target) in targets {
        for start in 0..4 {
            let arrived = mesh.walk_toward(NavTriRef(start), target).expect("a walk");
            assert_eq!(
                arrived, expected,
                "starting from {start} for {target} landed in {arrived}"
            );
        }
    }
}
