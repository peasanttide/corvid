//! The stored answer about a seam, against the derived one.

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

use corvid_nav::{NavError, NavMesh, NavTriRef, Tune};

use surface::{fold, metres, quad, ramp, scarp, strip};

/// Every seam of a mesh, checked both ways.
fn agrees(mesh: &NavMesh, tune: &Tune) {
    for (index, tri) in mesh.tris().iter().enumerate() {
        let reference = NavTriRef(u32::try_from(index).unwrap_or_default());
        for edge in 0..3usize {
            let Some(seam) = tri.edge(edge) else {
                assert_eq!(mesh.derive_walkable(reference, edge, tune), None);
                continue;
            };
            assert_eq!(
                Some(seam.is_walkable()),
                mesh.derive_walkable(reference, edge, tune),
                "{reference} edge {edge} to {}",
                seam.next()
            );
            assert_eq!(
                mesh.heights_agree(reference, edge, tune),
                Some(true),
                "two faces sharing vertices meet at the same height along {reference} edge {edge}"
            );
        }
    }
}

/// The precomputed flag is the derived one, on every seam of every fixture.
#[test]
fn what_was_precomputed_is_what_is_derivable() {
    let tune = Tune::default();
    agrees(&quad(), &tune);
    agrees(&ramp(), &tune);
    agrees(&fold(), &tune);
    agrees(&scarp(), &tune);
    agrees(&strip(3), &tune);
}

/// And it is derived rather than copied: a tune that allows less turns seams
/// off, and the derivation follows the tune it is given.
#[test]
fn the_derivation_follows_the_tune() {
    let mesh = ramp();
    let flat = Tune {
        max_slope: corvid_fixed::Angle16::from_degrees(10.0),
        ..Tune::default()
    };
    assert_eq!(
        mesh.derive_walkable(NavTriRef(0), 1, &flat),
        Some(false),
        "a 26.57 degree face is not walkable to a tune that stops at ten"
    );
    assert_eq!(
        mesh.derive_walkable(NavTriRef(0), 1, &Tune::default()),
        Some(true)
    );
}

/// A face steeper than the local frame can express is refused outright, which
/// is a different thing from being unwalkable.
#[test]
fn a_face_that_stands_on_its_edge_has_no_frame() {
    // A wall: three metres east, three metres up, no north at all.
    let vertices = [
        metres(0.0, 0.0, 0.0),
        metres(3.0, 0.0, 0.0),
        metres(0.0, 0.0, 3.0),
    ];
    assert_eq!(
        NavMesh::new(&vertices, &[[0, 1, 2]], &Tune::default()),
        Err(NavError::FaceTooSteep { face: 0 })
    );
}

/// The other two things a face can be refused for.
#[test]
fn a_face_is_refused_for_being_long_or_empty() {
    let far = [
        metres(0.0, 0.0, 0.0),
        metres(9.0, 0.0, 0.0),
        metres(0.0, 4.0, 0.0),
    ];
    assert_eq!(
        NavMesh::new(&far, &[[0, 1, 2]], &Tune::default()),
        Err(NavError::EdgeTooLong { face: 0 })
    );

    let flat = [metres(0.0, 0.0, 0.0), metres(3.0, 0.0, 0.0)];
    assert_eq!(
        NavMesh::new(&flat, &[[0, 1, 1]], &Tune::default()),
        Err(NavError::DegenerateFace { face: 0 })
    );

    assert_eq!(
        NavMesh::new(&flat, &[[0, 1, 5]], &Tune::default()),
        Err(NavError::VertexOutOfRange {
            face: 0,
            vertex: 5,
            count: 2
        })
    );
}

/// Three faces on one edge is not a partition of a surface, and a mesh that
/// claims to be one is refused rather than silently keeping two of them.
#[test]
fn an_edge_three_faces_share_is_refused() {
    let vertices = [
        metres(0.0, 0.0, 0.0),
        metres(3.0, 0.0, 0.0),
        metres(0.0, 3.0, 0.0),
        metres(3.0, 3.0, 0.0),
        metres(-3.0, 3.0, 0.0),
    ];
    assert_eq!(
        NavMesh::new(
            &vertices,
            &[[0, 1, 2], [3, 2, 1], [4, 2, 1]],
            &Tune::default()
        ),
        Err(NavError::NonManifoldEdge { from: 1, to: 2 })
    );
}

/// The grid keeps a triangle for the cell it covers most of, and a lookup
/// starts from it.
#[test]
fn the_grid_answers_for_the_ground_it_covers() {
    let mesh = strip(4);
    assert_eq!(mesh.grid().dims(), [2, 1, 1]);
    for column in 0..4 {
        let east = f64::from(column) * 3.0 + 1.0;
        let found = mesh.locate(metres(east, 1.0, 0.0)).expect("on the surface");
        let tri = mesh.tri(found.tri).expect("a triangle");
        let ecef = tri.ecef(found.decode().position);
        assert!(
            (ecef.x().to_f64() - east).abs() < 0.05,
            "{east} m east came back at {ecef}"
        );
        assert!(
            (ecef.y().to_f64() - 1.0).abs() < 0.05,
            "and one metre north: {ecef}"
        );
    }
}
