//! What a navigation mesh can refuse to be.

use crate::cords::NavTriRef;

/// Why a [`NavMesh`](crate::NavMesh) could not be built, or could not answer.
///
/// Every variant names a fact about the input rather than a step that failed,
/// because the caller's next move is to fix the mesh and the message is what
/// tells them which triangle to open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum NavError {
    /// A face names a vertex the vertex list does not have.
    #[error("face {face} names vertex {vertex}, past the {count} the mesh has")]
    VertexOutOfRange {
        /// The offending face's index.
        face: usize,
        /// The index it named.
        vertex: u32,
        /// How many vertices there are.
        count: usize,
    },

    /// Two of a face's three vertices are the same point, so it has no area.
    #[error("face {face} has no area")]
    DegenerateFace {
        /// The offending face's index.
        face: usize,
    },

    /// A face's plane is too close to containing the local up axis.
    ///
    /// Height is measured along the geocentric up, so a face standing on its
    /// edge has no height axis at all: its local frame is singular and no
    /// position on it can be written down. The determinant of the local frame
    /// is `2 * area * cos(slope)`, which is why steepness and degeneracy are
    /// the same failure here and not two.
    #[error("face {face} is steeper than the local frame can express")]
    FaceTooSteep {
        /// The offending face's index.
        face: usize,
    },

    /// A face has an edge longer than [`MAX_EDGE`](crate::MAX_EDGE).
    ///
    /// The local coordinates are eight bits across a whole triangle, so the
    /// edge length is what sets the resolution. A longer edge would quietly
    /// coarsen it.
    #[error("face {face} has an edge longer than the eight metres a local coordinate covers")]
    EdgeTooLong {
        /// The offending face's index.
        face: usize,
    },

    /// More than two faces share one edge, so the mesh is not a partition of a
    /// surface.
    #[error("the edge between vertices {from} and {to} is shared by more than two faces")]
    NonManifoldEdge {
        /// The lower of the two vertex indices.
        from: u32,
        /// The higher of the two vertex indices.
        to: u32,
    },

    /// The mesh spans more eight-metre cells than the grid is allowed to hold.
    #[error("the mesh needs {cells} grid cells, past the {limit} allowed")]
    GridTooLarge {
        /// How many cells covering the mesh would take.
        cells: u64,
        /// How many are allowed.
        limit: u64,
    },

    /// A triangle reference names a triangle this mesh does not have.
    #[error("{reference} is past the {count} triangles this mesh has")]
    UnknownTriangle {
        /// The reference that missed.
        reference: NavTriRef,
        /// How many triangles there are.
        count: usize,
    },

    /// A per-triangle field is not as long as the mesh.
    #[error("a field of {field} values cannot cover {tris} triangles")]
    FieldLengthMismatch {
        /// How many values the field has.
        field: usize,
        /// How many triangles the mesh has.
        tris: usize,
    },
}
