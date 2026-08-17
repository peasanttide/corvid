//! What a triangulation has to be true of, checked on the shapes that break
//! the different things: a square, a concave L, a ring with a hole in it, a
//! block with two courtyards, a concave hole in a concave block, and a
//! thirty-two point star.
#![allow(
    clippy::expect_used,
    reason = "a failed expect in a test is a failed test, which is what a test is for"
)]

use corvid_fixed::I48F16;
use corvid_geo::{GroundPoint, Polygon, Ring, Triangulate, Triangulation, Winding, ground};

/// Ten metres on a side, counterclockwise.
fn square() -> Polygon {
    Polygon::new(
        Ring::new(vec![
            ground(0, 0),
            ground(10, 0),
            ground(10, 10),
            ground(0, 10),
        ]),
        Vec::new(),
    )
}

/// The concave case: a twenty-metre L with one reflex corner, so an ear
/// search that never looks past convexity gets it wrong.
fn concave_l() -> Polygon {
    Polygon::new(
        Ring::new(vec![
            ground(0, 0),
            ground(20, 0),
            ground(20, 5),
            ground(5, 5),
            ground(5, 20),
            ground(0, 20),
        ]),
        Vec::new(),
    )
}

/// A block with a courtyard: thirty metres square with a ten-metre hole in
/// the middle of it.
fn ring_with_a_hole() -> Polygon {
    Polygon::new(
        Ring::new(vec![
            ground(0, 0),
            ground(30, 0),
            ground(30, 30),
            ground(0, 30),
        ]),
        vec![Ring::new(vec![
            ground(10, 10),
            ground(20, 10),
            ground(20, 20),
            ground(10, 20),
        ])],
    )
}

/// Every property a partition has to have, in one place, so each shape is
/// held to all of them.
fn assert_partitions(polygon: &Polygon, cut: &Triangulation) {
    assert_eq!(
        cut.area(),
        polygon.signed_area(),
        "the triangles cover the polygon and nothing else"
    );

    for &triangle in cut.triangles() {
        assert!(
            cut.triangle_area(triangle) > I48F16::ZERO,
            "triangle {triangle:?} is degenerate or wound backwards"
        );

        for index in triangle {
            let point = cut.points().get(index as usize).copied();
            assert!(point.is_some(), "index {index} names no point");
        }
    }

    // Every point a triangle names is one the polygon was built from, in the
    // order the rings were given, so a caller can carry its own per-point data
    // straight through the triangulation.
    let expected: Vec<GroundPoint> = polygon
        .outer()
        .points()
        .iter()
        .chain(polygon.holes().iter().flat_map(Ring::points))
        .copied()
        .collect();
    assert_eq!(cut.points(), expected);
}

#[test]
fn a_square_cuts_into_two_triangles() {
    let polygon = square();
    let cut = polygon.triangulate().expect("a square is a polygon");

    assert_eq!(cut.triangles().len(), 2);
    assert_eq!(polygon.signed_area(), I48F16::from_f64(100.0));
    assert_partitions(&polygon, &cut);
}

#[test]
fn a_concave_corner_is_not_an_ear() {
    let polygon = concave_l();
    let cut = polygon.triangulate().expect("an L is a polygon");

    // Six vertices, so four triangles, whatever order they come out in.
    assert_eq!(cut.triangles().len(), 4);
    assert_eq!(polygon.signed_area(), I48F16::from_f64(175.0));
    assert_partitions(&polygon, &cut);
}

#[test]
fn a_hole_is_bridged_and_the_courtyard_stays_empty() {
    let polygon = ring_with_a_hole();
    let cut = polygon.triangulate().expect("a courtyard is a hole");

    // Eight boundary vertices and one hole: `n + 2h - 2` triangles.
    assert_eq!(cut.triangles().len(), 8);
    assert_eq!(polygon.signed_area(), I48F16::from_f64(800.0));
    assert_partitions(&polygon, &cut);

    // The courtyard is not covered: no triangle contains its middle.
    let middle = ground(15, 15);
    for &triangle in cut.triangles() {
        let corners: Vec<GroundPoint> = triangle
            .iter()
            .filter_map(|&index| cut.points().get(index as usize).copied())
            .collect();
        let face = Ring::new(corners);
        assert!(
            !face.contains(middle) || face.on_boundary(middle),
            "a triangle covers the courtyard"
        );
    }
}

#[test]
fn triangulation_is_the_same_on_every_run() {
    let polygon = ring_with_a_hole();
    let once = polygon.triangulate().expect("a courtyard is a hole");
    let again = polygon.triangulate().expect("a courtyard is a hole");

    assert_eq!(once.triangles(), again.triangles());
}

#[test]
fn rings_are_reoriented_however_the_archive_stored_them() {
    let backwards = Ring::new(vec![
        ground(0, 10),
        ground(10, 10),
        ground(10, 0),
        ground(0, 0),
    ]);
    assert_eq!(backwards.winding(), Some(Winding::Clockwise));
    assert_eq!(backwards.signed_area(), I48F16::from_f64(-100.0));

    let polygon = Polygon::new(backwards, Vec::new());
    assert_eq!(polygon.outer().winding(), Some(Winding::Counterclockwise));
    assert_eq!(polygon.signed_area(), I48F16::from_f64(100.0));
}

#[test]
fn containment_is_by_winding_number_and_the_boundary_counts() {
    let polygon = ring_with_a_hole();

    assert!(polygon.contains(ground(5, 5)), "inside the block");
    assert!(!polygon.contains(ground(15, 15)), "inside the courtyard");
    assert!(!polygon.contains(ground(40, 15)), "outside altogether");
    assert!(polygon.contains(ground(0, 5)), "on the outer wall");
    assert!(polygon.contains(ground(10, 15)), "on the courtyard wall");
}

#[test]
fn a_ring_with_no_area_has_no_triangulation() {
    let collapsed = Polygon::new(
        Ring::new(vec![ground(0, 0), ground(10, 0), ground(20, 0)]),
        Vec::new(),
    );

    assert_eq!(collapsed.signed_area(), I48F16::ZERO);
    assert_eq!(collapsed.triangulate(), Err(Triangulate::Degenerate));
}

#[test]
fn a_hole_outside_its_ring_is_refused_rather_than_swallowed() {
    let wrong = Polygon::new(
        Ring::new(vec![
            ground(0, 0),
            ground(10, 0),
            ground(10, 10),
            ground(0, 10),
        ]),
        vec![Ring::new(vec![
            ground(20, 20),
            ground(25, 20),
            ground(25, 25),
            ground(20, 25),
        ])],
    );

    assert_eq!(wrong.triangulate(), Err(Triangulate::Unbridged));
}

#[test]
fn collinear_vertices_are_dropped_rather_than_triangulated() {
    // The same square, with a redundant point in the middle of one edge.
    let padded = Polygon::new(
        Ring::new(vec![
            ground(0, 0),
            ground(5, 0),
            ground(10, 0),
            ground(10, 10),
            ground(0, 10),
        ]),
        Vec::new(),
    );
    let cut = padded.triangulate().expect("a square with a marked edge");

    assert_eq!(cut.triangles().len(), 2);
    assert_partitions(&padded, &cut);
}

#[test]
fn two_courtyards_are_bridged_one_after_the_other() {
    let polygon = Polygon::new(
        Ring::new(vec![
            ground(0, 0),
            ground(60, 0),
            ground(60, 30),
            ground(0, 30),
        ]),
        vec![
            Ring::new(vec![
                ground(10, 10),
                ground(20, 10),
                ground(20, 20),
                ground(10, 20),
            ]),
            Ring::new(vec![
                ground(40, 10),
                ground(50, 10),
                ground(50, 20),
                ground(40, 20),
            ]),
        ],
    );
    let cut = polygon.triangulate().expect("two courtyards are two holes");

    assert_eq!(polygon.signed_area(), I48F16::from_f64(1600.0));
    assert_partitions(&polygon, &cut);
    assert!(!polygon.contains(ground(15, 15)));
    assert!(!polygon.contains(ground(45, 15)));
    assert!(polygon.contains(ground(30, 15)));
}

#[test]
fn a_courtyard_may_be_concave_too() {
    // An L-shaped light well in a concave block: reflex corners on both rings,
    // which is where a triangulator that only handles convex holes gives up.
    let polygon = Polygon::new(
        Ring::new(vec![
            ground(0, 0),
            ground(40, 0),
            ground(40, 40),
            ground(25, 40),
            ground(25, 25),
            ground(0, 25),
        ]),
        vec![Ring::new(vec![
            ground(5, 5),
            ground(20, 5),
            ground(20, 10),
            ground(10, 10),
            ground(10, 20),
            ground(5, 20),
        ])],
    );
    let cut = polygon
        .triangulate()
        .expect("an L in an L is still a polygon");

    // A 40 by 25 rectangle and a 15 by 15 head, less a 75 and a 50 well.
    assert_eq!(polygon.signed_area(), I48F16::from_f64(1100.0));
    assert_partitions(&polygon, &cut);
    assert!(!polygon.contains(ground(7, 7)), "inside the light well");
    assert!(polygon.contains(ground(15, 15)), "in the leg between them");
}

#[test]
fn a_long_ring_is_cut_without_losing_a_square_metre() {
    // A thirty-two point star: every other vertex reflex, which is the shape
    // that makes an ear search work for its answer.
    let mut points = Vec::new();
    for step in 0..32 {
        let radius = if step % 2 == 0 { 100 } else { 40 };
        let turn = f64::from(step) * core::f64::consts::TAU / 32.0;
        points.push(ground(
            corvid_fixed::I24F8::from_f64(f64::from(radius) * turn.cos()),
            corvid_fixed::I24F8::from_f64(f64::from(radius) * turn.sin()),
        ));
    }
    let polygon = Polygon::new(Ring::new(points), Vec::new());
    let cut = polygon.triangulate().expect("a star is a simple polygon");

    assert_eq!(cut.triangles().len(), 30);
    assert_partitions(&polygon, &cut);
}
