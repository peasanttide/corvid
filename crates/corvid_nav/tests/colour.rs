//! The colouring, and what makes it worth having.

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

use corvid_nav::{MAX_COLOURS, NavMesh, NavTriRef};

use surface::{fold, quad, ramp, scarp, sheet, strip};

/// No two triangles that share an edge share a colour, and every triangle is in
/// exactly one class.
fn holds(mesh: &NavMesh) -> usize {
    let colours = mesh.colours();
    for index in 0..mesh.len() {
        let reference = NavTriRef(u32::try_from(index).expect("a mesh of this size"));
        let mine = colours.colour_of(reference).expect("a colour");
        assert!(usize::from(mine) < MAX_COLOURS);
        for neighbour in mesh.neighbours(reference) {
            assert_ne!(
                colours.colour_of(neighbour),
                Some(mine),
                "{reference} and {neighbour} share an edge and a colour"
            );
        }
    }

    let mut seen = vec![0u32; mesh.len()];
    for class in colours.classes() {
        let mut previous: Option<NavTriRef> = None;
        for &reference in class {
            assert!(
                previous.is_none_or(|held| held.0 < reference.0),
                "a class is in triangle order"
            );
            previous = Some(reference);
            if let Some(slot) = seen.get_mut(reference.0 as usize) {
                *slot += 1;
            }
        }
    }
    assert!(
        seen.iter().all(|count| *count == 1),
        "the classes are a partition of the mesh"
    );
    colours.count()
}

/// Every fixture is coloured, and none of them needs more than the four a
/// triangle's three edges allow.
#[test]
fn no_two_triangles_across_a_seam_share_a_colour() {
    for mesh in [quad(), ramp(), fold(), scarp(), strip(6), sheet(8, 2.0)] {
        let count = holds(&mesh);
        assert!((1..=MAX_COLOURS).contains(&count));
    }
}

/// Ground triangulated in squares takes two colours, which is two passes of a
/// threaded tick: the two halves of a square are never neighbours of one
/// another's own colour, so the whole sheet alternates like a chessboard.
#[test]
fn a_sheet_takes_two_colours() {
    let mesh = sheet(12, 2.0);
    assert_eq!(holds(&mesh), 2);
    let classes: usize = mesh.colours().classes().map(<[NavTriRef]>::len).sum();
    assert_eq!(classes, mesh.len());
}

/// A lone triangle has nobody to disagree with.
#[test]
fn one_triangle_takes_one_colour() {
    let mesh = quad();
    assert_eq!(mesh.colours().colour_of(NavTriRef(0)), Some(0));
    assert_eq!(mesh.colours().colour_of(NavTriRef(9)), None);
}
