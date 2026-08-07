//! Indexed triangles, and the box they fit in.

use alloc::vec::Vec;

use crate::Vertex;
use corvid_fixed::{I16F16, I24F8};
use corvid_shape::Aabb;
use corvid_vector::globalpoint;

/// What a position component of [`Vertex::FULL`] is divided by on the way to
/// [`I24F8`] metres: the full deflection, times that type's own 256 steps to
/// the metre.
const PER_METRE: i64 = Vertex::FULL as i64 * 256;

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
    /// across sets this to 0.5 and puts its corners at `±FULL`.
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
            let [x, y, z] = vertex.position();
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
/// Rounded half away from zero, in `i64`, so the answer is the nearest
/// representable [`I24F8`] rather than whatever a truncation left. The widest
/// product is a full deflection against a full scale, which is 7.2e13 and fits
/// with fourteen bits to spare.
fn metres(component: i16, scale: I16F16) -> I24F8 {
    let numerator = i64::from(component) * i64::from(scale.to_bits());
    let half = PER_METRE / 2;
    let rounded = if numerator < 0 {
        (numerator - half) / PER_METRE
    } else {
        (numerator + half) / PER_METRE
    };
    I24F8::from_bits(i32::try_from(rounded).unwrap_or(if rounded < 0 {
        i32::MIN
    } else {
        i32::MAX
    }))
}
