//! Indexed triangles, and the box they fit in.

use alloc::vec::Vec;

use crate::Vertex;
use corvid_fixed::{I16F16, I24F8, Signed16};
use corvid_shape::Aabb;
use corvid_vector::globalpoint;

/// Indexed triangles, in the mesh's own space, with one scale for the lot.
///
/// The winding is counter-clockwise seen from outside, matching the
/// workspace's right-handed **+X right, +Y forward, +Z up** convention. That
/// is a convention rather than something this type enforces, and a mesh wound
/// the other way is drawn inside out by any pipeline that culls back faces.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Mesh {
    /// The vertices.
    pub vertices: Vec<Vertex>,
    /// Three indices per triangle, into [`vertices`](Self::vertices).
    pub indices: Vec<u32>,
    /// How many **metres** a position component of [`Vertex::FULL`] means.
    ///
    /// One number for the whole mesh, which is what keeps a vertex at twelve
    /// bytes: a per-vertex scale would be another attribute, and a per-axis one
    /// would waste the two axes a mesh is not longest along. A mesh a metre
    /// across sets this to 0.5 and puts its corners at `+/-FULL`.
    ///
    /// Nothing here applies it, apart from [`bounds`](Self::bounds). It reaches
    /// the device as whatever uniform the game's own shader reads, because a
    /// shader this crate did not write is the only kind there is now.
    pub scale: I16F16,
}

impl Mesh {
    /// A mesh from its three parts.
    #[must_use]
    pub const fn new(vertices: Vec<Vertex>, indices: Vec<u32>, scale: I16F16) -> Self {
        Self {
            vertices,
            indices,
            scale,
        }
    }

    /// The smallest axis-aligned box holding every vertex, **in metres**.
    ///
    /// This is the one place the two scales meet: a [`Vertex`] position is a
    /// signed fraction of [`Vertex::FULL`] and an [`Aabb`] is in `GlobalPoint`
    /// metres, so the conversion multiplies by [`scale`](Self::scale).
    ///
    /// A mesh with no vertices bounds nothing, which is [`Aabb::EMPTY`] rather
    /// than a point at the origin.
    ///
    /// ```
    /// use corvid_fixed::I16F16;
    /// use corvid_mesh::cube;
    ///
    /// let half = I16F16::from_f64(0.5);
    /// let bounds = cube(half).bounds();
    /// assert_eq!(bounds.max.x().to_f64(), 0.5);
    /// assert_eq!(bounds.min.z().to_f64(), -0.5);
    /// ```
    #[must_use]
    pub fn bounds(&self) -> Aabb {
        Aabb::from_points(self.vertices.iter().map(|vertex| {
            // As what they are: a position component is a signed share of the
            // mesh's scale, which is `Signed16`'s whole definition -- `[-1, 1]`
            // with `MIN == -MAX`, the SNORM asymmetry `Vertex::FULL` documents.
            let [x, y, z] = vertex.position().map(Signed16::from_bits);
            globalpoint(
                metres(x, self.scale),
                metres(y, self.scale),
                metres(z, self.scale),
            )
        }))
    }

    /// How many triangles there are, which is one per three indices.
    #[must_use]
    #[inline]
    pub const fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// Whether there is nothing to draw.
    ///
    /// About the indices rather than the vertices, because indices are what a
    /// draw call reads: a mesh carrying vertices no triangle names draws
    /// nothing, and this says so.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

/// One position component, in metres.
///
/// The component is a share of one and the scale is metres per full
/// deflection, so this is that share taken of that scale -- one
/// [`I16F16::saturating_mul_signed16`], which is the operation an analog axis
/// is scaled by too -- and then the narrowing onto the world's own type.
fn metres(component: Signed16, scale: I16F16) -> I24F8 {
    scale.saturating_mul_signed16(component).to_i24f8()
}
