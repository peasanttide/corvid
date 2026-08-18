//! The locating grid: in the level's own plane, sparse, and cell by cell.

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

use corvid_fixed::I24F8;
use corvid_nav::{NavGrid, NavMesh, NavTri, NavTriRef, Tune};

use surface::{apart, build, metres, sheet, world};

/// A district two and a half kilometres on a side, in forty-metre triangles.
///
/// This is the mesh the old grid could not hold. It was a dense three
/// dimensional array of eight-metre cells over the **ECEF** bounding box, and a
/// level plane at Paris's latitude lies across all three ECEF axes, so a square
/// district spent the `1 << 24` cell cap in every direction at once and stopped
/// at 2,464 m on a side. In the tangent plane at 32 m this is 78 by 78 cells
/// and the grid holds one entry per triangle per cell it reaches.
fn district(side: u32, pitch: f64) -> NavMesh {
    let mut vertices = Vec::new();
    for row in 0..=side {
        for column in 0..=side {
            vertices.push(metres(
                pitch * f64::from(column),
                pitch * f64::from(row),
                0.0,
            ));
        }
    }
    let stride = side + 1;
    let mut faces = Vec::new();
    for row in 0..side {
        for column in 0..side {
            let a = row * stride + column;
            faces.push([a, a + 1, a + stride]);
            faces.push([a + stride + 1, a + stride, a + 1]);
        }
    }
    build(&vertices, &faces)
}

/// The district builds, and every corner of it can be found.
#[test]
fn a_district_larger_than_the_old_ceiling_builds() {
    let mesh = district(64, 40.0);
    assert_eq!(mesh.len(), 64 * 64 * 2);
    let grid = mesh.grid();
    assert_eq!(grid.pitch(), NavGrid::DEFAULT_PITCH);

    for (east, north) in [
        (10.0, 10.0),
        (1280.0, 1280.0),
        (2540.0, 1000.0),
        (30.0, 2500.0),
    ] {
        let target = metres(east, north, 0.0);
        let found = mesh.locate(target).expect("somewhere on the district");
        let tri = mesh.tri(found.tri).expect("a triangle");
        assert!(
            NavTri::contains(tri.local(target)),
            "{east} m east and {north} m north landed in a triangle that does not hold it"
        );
        let there = world(&mesh, found.tri, found.decode().position);
        assert!(
            apart(there, target) < 0.05,
            "and the coordinates are the point: {there} against {target}"
        );
    }
}

/// The cells are the level's own squares: a point 32 m from another along the
/// ground is one cell from it, whatever ECEF thinks.
///
/// Which of the two axes a fixture's east falls on is the plane's business --
/// these surfaces sit over the equator at longitude zero, where the level's up
/// is the ECEF `Z` and the pair is a quarter turn from a bearing. What is
/// asserted is what a grid needs to be true: equal ground is equal cells, the
/// level starts at cell zero, and nothing about it is negative.
#[test]
fn the_cells_are_laid_out_in_the_levels_own_plane() {
    let mesh = district(8, 40.0);
    let grid = mesh.grid();
    let home = grid.cell_of(metres(4.0, 4.0, 0.0));

    let stepped = grid.cell_of(metres(4.0 + 32.0, 4.0, 0.0));
    let moved = [stepped.east - home.east, stepped.north - home.north];
    assert_eq!(
        moved.iter().map(|step| step.abs()).sum::<i32>(),
        1,
        "one pitch of ground is one cell: {moved:?}"
    );

    let further = grid.cell_of(metres(4.0, 4.0 + 96.0, 0.0));
    let moved = [further.east - home.east, further.north - home.north];
    assert_eq!(
        moved.iter().map(|step| step.abs()).sum::<i32>(),
        3,
        "and three pitches is three: {moved:?}"
    );

    for east in [0.0, 160.0, 320.0] {
        for north in [0.0, 160.0, 320.0] {
            let cell = grid.cell_of(metres(east, north, 0.0));
            assert!(
                cell.east >= 0 && cell.north >= 0,
                "the level starts at cell zero: {cell:?} at {east}, {north}"
            );
            assert!(cell.east <= 10 && cell.north <= 10, "and ends inside it");
        }
    }
}

/// A game may choose the pitch, and what it chooses is what it gets.
#[test]
fn the_pitch_is_the_tune_s() {
    let vertices = [
        metres(0.0, 0.0, 0.0),
        metres(60.0, 0.0, 0.0),
        metres(0.0, 60.0, 0.0),
        metres(60.0, 60.0, 0.0),
    ];
    let tune = Tune {
        grid_pitch: I24F8::from_f64(16.0),
        ..Tune::default()
    };
    let mesh = NavMesh::new(&vertices, &[[0, 1, 2], [3, 2, 1]], &tune).expect("a surface");
    assert_eq!(mesh.grid().pitch(), I24F8::from_f64(16.0));
    // Sixty metres over sixteen is four cells each way, and both triangles
    // reach every one of the sixteen.
    assert_eq!(mesh.grid().len(), 2 * 16);
}

/// One cell can be re-cut without re-cutting the world.
#[test]
fn a_cell_rebuilds_on_its_own() {
    let mesh = sheet(8, 2.0);
    let mut grid = mesh.grid().clone();
    let cell = grid.cell_of(metres(1.0, 1.0, 0.0));
    let before: Vec<NavTriRef> = grid.tris_in(cell).collect();
    assert!(before.len() > 1, "the sheet is inside one cell");

    // Nothing has changed, so re-cutting the cell changes nothing.
    grid.rebuild_cell(mesh.tris(), cell, &[]);
    assert_eq!(grid.tris_in(cell).collect::<Vec<_>>(), before);
    assert_eq!(&grid, mesh.grid());

    // The editor has deleted the tail of the mesh. Only the cell it re-cuts
    // hears about it, and only the triangles that are gone leave.
    let kept = mesh.len() - 4;
    grid.rebuild_cell(&mesh.tris()[..kept], cell, &[]);
    let after: Vec<NavTriRef> = grid.tris_in(cell).collect();
    assert_eq!(after.len(), before.len() - 4);
    assert!(after.iter().all(|tri| (tri.0 as usize) < kept));
}
