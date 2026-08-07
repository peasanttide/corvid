//! The four properties every generator holds.
//!
//! They are the four that catch a generator that is *wrong* rather than merely
//! different from the one somebody had in mind: an index that names nothing, a
//! face wound inside out, a normal that belongs to another face, and a mesh
//! that does not fill the box its scale claims. Each is written once, over a
//! table, so a seventh generator is a table entry rather than four more tests.
//!
//! The arithmetic here is `f64` and that is on purpose. A generator is
//! client-ring data — nothing it produces is hashed, sent or replayed — so what
//! these check is geometry rather than bit patterns, and a cross product in
//! floating point says what a cross product in fixed point would say without a
//! second implementation of the maths under test.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::suboptimal_flops,
    reason = "the cross product below is written out because it is the thing being checked against, and a fused multiply-add would make this a test of a different expression from the one a generator's own integer cross product implements"
)]

use corvid_fixed::I16F16;

use corvid_mesh::{Mesh, Vertex, cone, cube, cylinder, grid, icosphere, quad};
/// How **outside** is decided for one mesh.
enum Outside {
    /// A closed mesh with the origin strictly inside it. Every one of the four
    /// closed generators is convex about its own centre, so a face points
    /// outward exactly when its normal points away from the origin.
    Origin,
    /// A flat mesh has no inside to be outside of, so every face points the one
    /// way it can.
    Toward([f64; 3]),
}

/// One generator's output, and what is true of it.
struct Generated {
    /// What to name in a failure.
    name: &'static str,
    /// The mesh itself.
    mesh: Mesh,
    /// How its faces are checked for winding.
    outside: Outside,
    /// The axes it reaches ±[`Vertex::FULL`] on, at **both** ends.
    ///
    /// Not every axis of every mesh: a quad is flat in `z`, and an icosahedron
    /// with no subdivision has vertices only at its poles and on two rings, so
    /// its equator misses every axis. What the property says is that a mesh
    /// spends its position range rather than sitting inside it, and this is
    /// where each generator states which axes carry that.
    full: &'static [usize],
}

/// One of every generator, at sizes that make the table above true.
fn generators() -> Vec<Generated> {
    let one = I16F16::ONE;
    vec![
        Generated {
            name: "cube",
            mesh: cube(I16F16::from_f64(0.5)),
            outside: Outside::Origin,
            full: &[0, 1, 2],
        },
        Generated {
            name: "quad",
            mesh: quad(one),
            outside: Outside::Toward([0.0, 0.0, 1.0]),
            full: &[0, 1],
        },
        Generated {
            name: "grid",
            mesh: grid(one, 4),
            outside: Outside::Toward([0.0, 0.0, 1.0]),
            full: &[0, 1],
        },
        Generated {
            // Subdivided at least once, because that is what puts a vertex on
            // an axis: the ten equator vertices a first subdivision makes are
            // the midpoints of the upper-lower edges, whose two heights cancel.
            name: "icosphere",
            mesh: icosphere(one, 2),
            outside: Outside::Origin,
            full: &[0, 1, 2],
        },
        Generated {
            // Eight sides, so the ring lands on all four of `±X` and `±Y`.
            name: "cylinder",
            mesh: cylinder(one, one, 8),
            outside: Outside::Origin,
            full: &[0, 1, 2],
        },
        Generated {
            name: "cone",
            mesh: cone(one, one, 8),
            outside: Outside::Origin,
            full: &[0, 1, 2],
        },
    ]
}

/// The angle two unit vectors may differ by and still be called the same
/// direction, as a cosine.
///
/// Two degrees. [`corvid_vector::OctDirection`]'s own worst error is 0.9569°,
/// measured, and the rest of the margin is the position quantization the face
/// normal is computed from — a triangle on a subdivided sphere is small, and a
/// last-bit move in a corner turns its plane by more than it turns a big one's.
const AGREEMENT: f64 = 0.999_390;

/// Every index names a vertex that exists, and the indices come in threes.
#[test]
fn every_index_is_in_range() {
    for Generated { name, mesh, .. } in generators() {
        assert!(!mesh.is_empty(), "{name} generated nothing");
        assert_eq!(
            mesh.indices.len() % 3,
            0,
            "{name} has {} indices, which is not a whole number of triangles",
            mesh.indices.len(),
        );
        let count = u32::try_from(mesh.vertices.len()).unwrap();
        for (position, index) in mesh.indices.iter().enumerate() {
            assert!(
                *index < count,
                "{name}'s index {position} names vertex {index} of {count}",
            );
        }
    }
}

/// Every face is wound counter-clockwise seen from outside, which is what the
/// workspace's convention says and what a back-face-culling pipeline needs.
///
/// Checked by the sign of the face normal against the vector from the mesh's
/// centre to the face, which is the test that separates a mesh drawn solid from
/// one drawn inside out.
#[test]
fn every_face_winds_outward() {
    for Generated {
        name,
        mesh,
        outside,
        ..
    } in generators()
    {
        for (face, corners) in triangles(&mesh).enumerate() {
            let normal = cross(corners);
            let outward = match outside {
                Outside::Origin => centroid(corners),
                Outside::Toward(direction) => direction,
            };
            assert!(
                dot(normal, outward) > 0.0,
                "{name}'s face {face} is wound inside out: normal {normal:?}, outward \
                 {outward:?}",
            );
        }
    }
}

/// Every vertex's stored normal agrees with the face it belongs to.
///
/// That is what flat-shaded means, and it is the property a shared-corner
/// mistake breaks: a vertex reused by two faces that are not coplanar can only
/// carry one of the two normals, so one of them comes out wrong.
#[test]
fn every_normal_matches_its_face() {
    for Generated { name, mesh, .. } in generators() {
        for (face, indices) in mesh.indices.chunks_exact(3).enumerate() {
            let corners = corners_of(&mesh, indices);
            let plane = normalized(cross(corners)).expect("a face with no plane");
            for index in indices {
                let stored = mesh.vertices[*index as usize].normal().decode().to_array();
                let stored = [stored[0].to_f64(), stored[1].to_f64(), stored[2].to_f64()];
                assert!(
                    dot(stored, plane) > AGREEMENT,
                    "{name}'s vertex {index} on face {face} stores {stored:?} where its face \
                     faces {plane:?}",
                );
            }
        }
    }
}

/// The extremes reach `±Vertex::FULL` exactly, so the mesh fills the box its
/// `scale` claims rather than sitting inside it, and nothing runs past it.
#[test]
fn the_extremes_are_full_scale() {
    for Generated {
        name, mesh, full, ..
    } in generators()
    {
        let mut low = [i16::MAX; 3];
        let mut high = [i16::MIN; 3];
        for vertex in &mesh.vertices {
            let position = vertex.position();
            for axis in 0..3 {
                low[axis] = low[axis].min(position[axis]);
                high[axis] = high[axis].max(position[axis]);
                assert!(
                    position[axis] >= -Vertex::FULL,
                    "{name} has a component of {} on axis {axis}, past the negative end                      `SNORM` can represent",
                    position[axis],
                );
            }
        }
        assert!(!full.is_empty(), "{name} claims to fill no axis at all");
        for axis in full {
            assert_eq!(
                (low[*axis], high[*axis]),
                (-Vertex::FULL, Vertex::FULL),
                "{name} spans {} to {} on axis {axis} rather than the whole of it",
                low[*axis],
                high[*axis],
            );
        }
    }
}

/// Every triangle of a mesh, as three positions.
fn triangles(mesh: &Mesh) -> impl Iterator<Item = [[f64; 3]; 3]> + '_ {
    mesh.indices
        .chunks_exact(3)
        .map(|indices| corners_of(mesh, indices))
}

/// The three positions one triangle's indices name.
fn corners_of(mesh: &Mesh, indices: &[u32]) -> [[f64; 3]; 3] {
    let corner = |index: u32| {
        let position = mesh.vertices[index as usize].position();
        [
            f64::from(position[0]),
            f64::from(position[1]),
            f64::from(position[2]),
        ]
    };
    [corner(indices[0]), corner(indices[1]), corner(indices[2])]
}

/// `(b - a) × (c - a)`, which points outward for a triangle wound
/// counter-clockwise seen from outside.
fn cross([first, second, third]: [[f64; 3]; 3]) -> [f64; 3] {
    let (along, across) = (
        [
            second[0] - first[0],
            second[1] - first[1],
            second[2] - first[2],
        ],
        [
            third[0] - first[0],
            third[1] - first[1],
            third[2] - first[2],
        ],
    );
    [
        along[1] * across[2] - along[2] * across[1],
        along[2] * across[0] - along[0] * across[2],
        along[0] * across[1] - along[1] * across[0],
    ]
}

/// The middle of a triangle, which is a point on it that no corner is.
fn centroid([first, second, third]: [[f64; 3]; 3]) -> [f64; 3] {
    [
        (first[0] + second[0] + third[0]) / 3.0,
        (first[1] + second[1] + third[1]) / 3.0,
        (first[2] + second[2] + third[2]) / 3.0,
    ]
}

/// The same vector at unit length, or [`None`] for one with no length.
fn normalized(vector: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(vector, vector).sqrt();
    if length > 0.0 {
        Some([vector[0] / length, vector[1] / length, vector[2] / length])
    } else {
        None
    }
}

/// The dot product.
fn dot(one: [f64; 3], other: [f64; 3]) -> f64 {
    one[0] * other[0] + one[1] * other[1] + one[2] * other[2]
}
