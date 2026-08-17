//! The surfaces the tests walk on.
//!
//! Every one of them sits on the earth's surface rather than at the origin,
//! because the local up is the direction of the centroid from the earth's
//! centre and a mesh at the origin has no up at all. A patch a few metres
//! across at this radius leans by about three ten-millionths of a radian, which
//! is far below a position code and is the correct answer for a patch of a
//! sphere.

use corvid_fixed::{Factor16, I16F16, I24F8};
use corvid_nav::{NavMesh, NavTriRef, Tune};
use corvid_vector::{FinePoint, GlobalPoint};

/// The earth's radius, which is where a level is.
pub const RADIUS: f64 = 6_371_000.0;

/// A world position, in metres east, north and up from a point on the equator.
pub fn metres(east: f64, north: f64, up: f64) -> GlobalPoint {
    GlobalPoint::new(
        I24F8::from_f64(east),
        I24F8::from_f64(north),
        I24F8::from_f64(RADIUS + up),
    )
}

/// A tune with the world switched off, for tests about geometry alone.
pub fn inert() -> Tune {
    Tune {
        gravity: I16F16::ZERO,
        drag: Factor16::MIN,
        ..Tune::default()
    }
}

/// Two triangles making a four-metre square of level ground.
///
/// Face 0 is `[0, 1, 2]` and face 1 is `[3, 2, 1]`, so the seam between them is
/// edge 1 of each and the square's four outer edges have no neighbour at all --
/// which is what makes this fixture a cliff as well as a quad.
pub fn quad() -> NavMesh {
    let vertices = [
        metres(0.0, 0.0, 0.0),
        metres(4.0, 0.0, 0.0),
        metres(0.0, 4.0, 0.0),
        metres(4.0, 4.0, 0.0),
    ];
    build(&vertices, &[[0, 1, 2], [3, 2, 1]])
}

/// Two triangles making a four-metre square that rises two metres northward,
/// which is a slope of 26.57 degrees.
pub fn ramp() -> NavMesh {
    let vertices = [
        metres(0.0, 0.0, 0.0),
        metres(4.0, 0.0, 0.0),
        metres(0.0, 4.0, 2.0),
        metres(4.0, 4.0, 2.0),
    ];
    build(&vertices, &[[0, 1, 2], [3, 2, 1]])
}

/// Four triangles: a flat shelf and a scarp rising five metres over two.
///
/// The scarp leans 68 degrees, which is inside the 80 the local frame needs and
/// well outside the 50 a walker is allowed, so the seam between them is
/// walkable in one direction and not in the other. That asymmetry is the rule
/// working: what stops a walker is the slope of the face being stepped onto.
pub fn scarp() -> NavMesh {
    let vertices = [
        metres(0.0, 0.0, 0.0),
        metres(3.0, 0.0, 0.0),
        metres(0.0, 2.0, 0.0),
        metres(3.0, 2.0, 0.0),
        metres(0.0, 4.0, 5.0),
        metres(3.0, 4.0, 5.0),
    ];
    build(&vertices, &[[0, 1, 2], [3, 2, 1], [2, 3, 4], [5, 4, 3]])
}

/// Four triangles making a valley: a panel falling two metres northward and
/// another rising two metres back.
///
/// The whole thing fits one grid cell, so the grid has exactly one guess to
/// offer and it is wrong for three quarters of the surface.
pub fn fold() -> NavMesh {
    let vertices = [
        metres(0.0, 0.0, 2.0),
        metres(3.0, 0.0, 2.0),
        metres(0.0, 3.0, 0.0),
        metres(3.0, 3.0, 0.0),
        metres(0.0, 6.0, 2.0),
        metres(3.0, 6.0, 2.0),
    ];
    build(&vertices, &[[0, 1, 2], [3, 2, 1], [2, 3, 4], [5, 4, 3]])
}

/// A row of `quads` three-metre squares of level ground, running east.
///
/// Triangle `2k` and triangle `2k + 1` are the two halves of square `k`, and
/// consecutive squares share an edge, so the whole strip is one connected chain
/// -- which is what a diffusion has to spread along.
pub fn strip(quads: u32) -> NavMesh {
    let mut vertices = Vec::new();
    for column in 0..=quads {
        let east = 3.0 * f64::from(column);
        vertices.push(metres(east, 0.0, 0.0));
        vertices.push(metres(east, 3.0, 0.0));
    }
    let mut faces = Vec::new();
    for column in 0..quads {
        let base = 2 * column;
        faces.push([base, base + 2, base + 1]);
        faces.push([base + 3, base + 1, base + 2]);
    }
    build(&vertices, &faces)
}

/// A mesh, or the test's failure.
pub fn build(vertices: &[GlobalPoint], faces: &[[u32; 3]]) -> NavMesh {
    NavMesh::new(vertices, faces, &Tune::default()).expect("the fixture is not a surface")
}

/// Where a set of local coordinates is in the world.
pub fn world(mesh: &NavMesh, tri: NavTriRef, local: FinePoint) -> GlobalPoint {
    mesh.tri(tri).expect("no such triangle").ecef(local)
}

/// How far apart two world positions are, in metres.
pub fn apart(left: GlobalPoint, right: GlobalPoint) -> f64 {
    left.distance(right).to_f64()
}

/// A local velocity read back as metres per second along the world axes.
pub fn world_velocity(mesh: &NavMesh, tri: NavTriRef, local: FinePoint) -> [f64; 3] {
    let moved = mesh
        .tri(tri)
        .expect("no such triangle")
        .local_to_ecef()
        .apply(local);
    [moved.x().to_f64(), moved.y().to_f64(), moved.z().to_f64()]
}
