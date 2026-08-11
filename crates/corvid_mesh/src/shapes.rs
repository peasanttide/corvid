//! The generators: six shapes, all flat-shaded.
//!
//! **Flat-shaded, always.** Every generator here emits one vertex per face
//! corner rather than sharing corners between faces, because a device has no
//! per-face storage -- a normal is a vertex attribute or it is nothing -- and
//! because the faceted look is what these are for. A caller that wants smooth
//! normals is generating a different mesh, not post-processing one of these.
//!
//! Every one of them fills the box its [`Mesh::scale`] claims: the extremes sit
//! at +/-[`Vertex::FULL`] on at least one axis, so none of the sixteen bits a
//! position component has is spent on empty space.

use alloc::vec::Vec;

use corvid_fixed::I16F16;
use corvid_vector::OctDirection;

use crate::geometry::{
    circle, division, face_normal, fraction, halfway, icosahedron, larger, on_sphere, unit,
};
use crate::{Mesh, Vertex};

/// The largest [`grid`] a caller gets, per side.
///
/// 256 cells a side is 131072 triangles and 262144 vertices, which is three
/// megabytes of vertex buffer. A mesh past that wants a representation with a
/// level of detail in it rather than a bigger `u16`.
const CELLS: u16 = 256;

/// The most times [`icosphere`] subdivides.
///
/// Each subdivision quadruples the face count, so four is 5120 triangles from
/// the twenty an icosahedron starts with. The argument is a `u8` and 255 would
/// be a number with a hundred and fifty digits in it.
const SUBDIVISIONS: u8 = 4;

/// The fewest sides [`cylinder`] and [`cone`] will turn a circle into.
const FEWEST: u16 = 3;

/// The most.
const MOST: u16 = 1024;

/// A cube `half` metres from its centre to each face, flat-shaded, wound
/// counter-clockwise seen from outside.
///
/// Twenty-four vertices rather than eight, because a face's normal belongs to
/// the face: a shared corner would have to average the three faces that meet
/// there, and the whole look is that it does not. Each face is built from a
/// tangent and a bitangent whose cross product is the outward normal, so the
/// winding is right by construction rather than by six copied index lists.
///
/// Every corner sits at +/-[`Vertex::FULL`], and `half` is the mesh's scale --
/// so a position component of one means `half` metres and the cube is exactly
/// twice that across.
///
/// ```
/// use corvid_fixed::I16F16;
/// use corvid_mesh::cube;
///
/// let metre = cube(I16F16::from_f64(0.5));
/// assert_eq!(metre.vertices.len(), 24);
/// assert_eq!(metre.triangles(), 12);
/// ```
#[must_use]
pub fn cube(half: I16F16) -> Mesh {
    /// Each face as its outward normal, a tangent and a bitangent, in that
    /// order, with `tangent x bitangent = normal`.
    const FACES: [([i32; 3], [i32; 3], [i32; 3]); 6] = [
        ([1, 0, 0], [0, 1, 0], [0, 0, 1]),
        ([-1, 0, 0], [0, 0, 1], [0, 1, 0]),
        ([0, 1, 0], [0, 0, 1], [1, 0, 0]),
        ([0, -1, 0], [1, 0, 0], [0, 0, 1]),
        ([0, 0, 1], [1, 0, 0], [0, 1, 0]),
        ([0, 0, -1], [0, 1, 0], [1, 0, 0]),
    ];

    let mut faces = Faces::with_capacity(24, 36);
    for (normal, tangent, bitangent) in FACES {
        // A face normal is an axis, so its components are already the ratio
        // `from_ratio` wants -- no float, and no `Signed32` spelled out.
        let facing = OctDirection::encode(unit([
            i64::from(normal[0]),
            i64::from(normal[1]),
            i64::from(normal[2]),
        ]));
        let corner = |along: i32, across: i32| {
            let mut position = [0i16; 3];
            for (axis, component) in position.iter_mut().enumerate() {
                // A cube's corner is at full deflection on every axis, so each
                // sum is +/-1 and there is no scaling to do -- only the choice
                // of which end of the range it names.
                *component = match normal[axis] + along * tangent[axis] + across * bitangent[axis] {
                    1 => Vertex::FULL,
                    -1 => -Vertex::FULL,
                    _ => 0,
                };
            }
            position
        };
        faces.quad(
            [corner(-1, -1), corner(1, -1), corner(1, 1), corner(-1, 1)],
            facing,
        );
    }
    faces.into_mesh(half)
}

/// One square facing **+Z**, `half` metres from its centre to each edge.
///
/// The floor, the wall, the billboard and the shadow catcher. Its four corners
/// sit at +/-[`Vertex::FULL`] in `x` and `y` and at zero in `z`, so it is flat
/// in the mesh's own space as well as in metres.
#[must_use]
pub fn quad(half: I16F16) -> Mesh {
    let mut faces = Faces::with_capacity(4, 6);
    let reach = Vertex::FULL;
    faces.quad(
        [
            [-reach, -reach, 0],
            [reach, -reach, 0],
            [reach, reach, 0],
            [-reach, reach, 0],
        ],
        OctDirection::UP,
    );
    faces.into_mesh(half)
}

/// A [`quad`] cut into `cells` by `cells` squares.
///
/// The same surface and the same normal; what the extra vertices buy is a
/// vertex shader that can displace them. `cells` is clamped to at least one and
/// at most 256: 256 a side is 131072 triangles and three megabytes of vertex
/// buffer, and a mesh past that wants a representation with a level of detail
/// in it rather than a bigger `u16`.
#[must_use]
pub fn grid(half: I16F16, cells: u16) -> Mesh {
    let cells = u32::from(cells.clamp(1, CELLS));
    let quads = (cells * cells) as usize;
    let mut faces = Faces::with_capacity(quads * 4, quads * 6);
    for row in 0..cells {
        for column in 0..cells {
            let (left, right) = (division(column, cells), division(column + 1, cells));
            let (near, far) = (division(row, cells), division(row + 1, cells));
            faces.quad(
                [
                    [left, near, 0],
                    [right, near, 0],
                    [right, far, 0],
                    [left, far, 0],
                ],
                OctDirection::UP,
            );
        }
    }
    faces.into_mesh(half)
}

/// A sphere of `radius` metres, as a subdivided icosahedron.
///
/// The poles are on `+/-Z`, which is what puts the extremes at
/// +/-[`Vertex::FULL`] exactly: the usual golden-ratio orientation has no
/// vertex on any axis, so it would sit 15% inside the box its scale claims.
///
/// `subdivisions` is clamped to at most four, because each one quadruples the
/// face count and the argument is a `u8`. Zero is the icosahedron itself,
/// twenty triangles; four is 5120.
///
/// ```
/// use corvid_fixed::I16F16;
/// use corvid_mesh::icosphere;
///
/// assert_eq!(icosphere(I16F16::ONE, 0).triangles(), 20);
/// assert_eq!(icosphere(I16F16::ONE, 2).triangles(), 320);
/// ```
#[must_use]
pub fn icosphere(radius: I16F16, subdivisions: u8) -> Mesh {
    let mut triangles = icosahedron();
    for _ in 0..subdivisions.min(SUBDIVISIONS) {
        let mut finer = Vec::with_capacity(triangles.len() * 4);
        for [a, b, c] in triangles {
            let (ab, bc, ca) = (halfway(a, b), halfway(b, c), halfway(c, a));
            finer.extend_from_slice(&[[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
        }
        triangles = finer;
    }

    let mut faces = Faces::with_capacity(triangles.len() * 3, triangles.len() * 3);
    for corners in triangles {
        faces.triangle(corners.map(|corner| on_sphere(corner, Vertex::FULL)));
    }
    faces.into_mesh(radius)
}

/// A closed cylinder about the **Z** axis, `radius` metres across and
/// `half_height` metres from its middle to each cap.
///
/// `sides` is clamped to between three and 1024. The mesh's scale is the larger
/// of the two measurements, so the longer axis reaches
/// +/-[`Vertex::FULL`] and the shorter one is a fraction of it.
#[must_use]
pub fn cylinder(radius: I16F16, half_height: I16F16, sides: u16) -> Mesh {
    let sides = u32::from(sides.clamp(FEWEST, MOST));
    let scale = larger(radius, half_height);
    let (across, up) = (fraction(radius, scale), fraction(half_height, scale));
    let ring = circle(sides, across);

    let quads = sides as usize;
    let mut faces = Faces::with_capacity(quads * 10, quads * 12);
    for step in 0..quads {
        let (here, next) = (ring[step], ring[(step + 1) % quads]);
        faces.quad(
            [
                [here[0], here[1], -up],
                [next[0], next[1], -up],
                [next[0], next[1], up],
                [here[0], here[1], up],
            ],
            face_normal(
                [here[0], here[1], -up],
                [next[0], next[1], -up],
                [next[0], next[1], up],
            ),
        );
        faces.triangle([[0, 0, up], [here[0], here[1], up], [next[0], next[1], up]]);
        faces.triangle([
            [0, 0, -up],
            [next[0], next[1], -up],
            [here[0], here[1], -up],
        ]);
    }
    faces.into_mesh(scale)
}

/// A closed cone about the **Z** axis, apex at `+Z`.
///
/// `radius` is the base's, `half_height` is from the middle to the apex and to
/// the base alike, and `sides` is clamped the way [`cylinder`]'s is. The mesh's
/// scale is the larger of the two measurements, for the same reason.
#[must_use]
pub fn cone(radius: I16F16, half_height: I16F16, sides: u16) -> Mesh {
    let sides = u32::from(sides.clamp(FEWEST, MOST));
    let scale = larger(radius, half_height);
    let (across, up) = (fraction(radius, scale), fraction(half_height, scale));
    let ring = circle(sides, across);

    let count = sides as usize;
    let mut faces = Faces::with_capacity(count * 6, count * 6);
    for step in 0..count {
        let (here, next) = (ring[step], ring[(step + 1) % count]);
        faces.triangle([[0, 0, up], [here[0], here[1], -up], [next[0], next[1], -up]]);
        faces.triangle([
            [0, 0, -up],
            [next[0], next[1], -up],
            [here[0], here[1], -up],
        ]);
    }
    faces.into_mesh(scale)
}

/// Vertices and indices, with the flat-shading rule built in.
///
/// Every method here pushes fresh vertices, so nothing a caller does can end up
/// sharing a corner between two faces that do not share a plane.
struct Faces {
    /// The vertices so far.
    vertices: Vec<Vertex>,
    /// The indices so far.
    indices: Vec<u32>,
}

impl Faces {
    /// Room for a mesh of a known size.
    fn with_capacity(vertices: usize, indices: usize) -> Self {
        Self {
            vertices: Vec::with_capacity(vertices),
            indices: Vec::with_capacity(indices),
        }
    }

    /// Where the next face's vertices begin.
    fn base(&self) -> u32 {
        u32::try_from(self.vertices.len()).unwrap_or(0)
    }

    /// One triangle, wound as given, with the normal its own plane implies.
    fn triangle(&mut self, corners: [[i16; 3]; 3]) {
        let facing = face_normal(corners[0], corners[1], corners[2]);
        let base = self.base();
        for corner in corners {
            self.vertices.push(Vertex::new(corner, facing));
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    /// One planar quad, wound as given, as two triangles over four corners.
    ///
    /// The four are shared between the two triangles and by nothing else, which
    /// is what flat-shading a quad means: both halves lie in one plane, so one
    /// normal is right for both.
    fn quad(&mut self, corners: [[i16; 3]; 4], facing: OctDirection) {
        let base = self.base();
        for corner in corners {
            self.vertices.push(Vertex::new(corner, facing));
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// The mesh, at `scale` metres to a full position component.
    fn into_mesh(self, scale: I16F16) -> Mesh {
        Mesh::new(self.vertices, self.indices, scale)
    }
}
