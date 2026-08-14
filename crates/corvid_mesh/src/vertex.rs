//! One vertex, in twelve bytes.

use corvid_vector::OctDirection;

/// One vertex: **twelve bytes**, against the twenty-four a float vertex costs.
///
/// | | |
/// |---|---|
/// | Position | three `i16`, read as `Snorm16x4` at offset 0 |
/// | Normal | [`OctDirection`], read as `Snorm8x2` at offset 8 |
/// | Stride | 12 |
///
/// # Why the position is `i16`
///
/// Positions inside a mesh are relative to the mesh's own origin, and sixteen
/// bits over a mesh-sized box is finer than anything a player can see: a metre
/// wide cube resolves to 15 um. What that buys is memory bandwidth at fifty
/// thousand instances, where the precision was never the limit.
///
/// The components are `SNORM`, so the device reads them as `[-1, 1]` and the
/// mesh's own [`scale`](crate::Mesh::scale) is what turns them into metres.
/// That division is free -- it is what the hardware's `Snorm16` conversion
/// already does -- where a `Sint16` position would cost the vertex shader a
/// multiply by a reciprocal the game had to supply anyway.
///
/// # The fourth component
///
/// There is no three-component sixteen-bit vertex format in WebGPU: the widths
/// go `Snorm16x2` and then `Snorm16x4`. So a three-component position is read
/// as four and the fourth component is padding, and is always zero. Two more
/// bytes after the normal bring the stride to twelve,
/// which is what an array stride being a multiple of four requires.
///
/// It is worth knowing rather than hiding, because it is the difference
/// between the eight bytes the three fields weigh and the twelve a vertex
/// buffer actually costs.
///
/// # Examples
///
/// ```
/// use corvid_mesh::Vertex;
/// use corvid_vector::OctDirection;
///
/// let corner = Vertex::new([Vertex::FULL, -Vertex::FULL, Vertex::FULL], OctDirection::UP);
/// assert_eq!(size_of::<Vertex>(), 12);
/// assert_eq!(corner.position(), [Vertex::FULL, -Vertex::FULL, Vertex::FULL]);
/// assert_eq!(corner.normal(), OctDirection::UP);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// Where it is, in the mesh's own space, as a signed fraction of
    /// [`FULL`](Self::FULL). The fourth component is padding, for the reason
    /// this type's own documentation gives.
    position: [i16; 4],
    /// Which way the surface faces there, octahedrally encoded.
    ///
    /// Per-vertex rather than per-face, because a device has no per-face
    /// storage. A flat-shaded mesh is one whose faces do not share vertices,
    /// so [`cube`](crate::cube) has twenty-four vertices rather than eight.
    normal: OctDirection,
    /// The two bytes that bring the stride to twelve. Always zero.
    pad: [u8; 2],
}

impl Vertex {
    /// The position component that means the mesh's full
    /// [`scale`](crate::Mesh::scale).
    ///
    /// 32767 rather than 32768, because that is what `SNORM` maps to exactly
    /// one: `-32768` and `-32767` both decode to `-1.0`, which is the one
    /// asymmetry in the format and the reason a mesh's extremes are written as
    /// `+/-FULL`.
    pub const FULL: i16 = i16::MAX;

    /// A vertex at `position`, whose surface faces along `normal`.
    #[must_use]
    #[inline]
    pub const fn new(position: [i16; 3], normal: OctDirection) -> Self {
        Self {
            position: [position[0], position[1], position[2], 0],
            normal,
            pad: [0; 2],
        }
    }

    /// Where it is, without the padding component.
    #[must_use]
    #[inline]
    pub const fn position(self) -> [i16; 3] {
        [self.position[0], self.position[1], self.position[2]]
    }

    /// Which way the surface faces.
    #[must_use]
    #[inline]
    pub const fn normal(self) -> OctDirection {
        self.normal
    }
}

/// A vertex from its two parts, which is [`Vertex::new`].
impl From<([i16; 3], OctDirection)> for Vertex {
    #[inline]
    fn from((position, normal): ([i16; 3], OctDirection)) -> Self {
        Self::new(position, normal)
    }
}

/// The two parts a vertex is, without the padding component.
impl From<Vertex> for ([i16; 3], OctDirection) {
    #[inline]
    fn from(vertex: Vertex) -> Self {
        (vertex.position(), vertex.normal())
    }
}
