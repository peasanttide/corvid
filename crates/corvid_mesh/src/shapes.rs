//! The generators: six shapes, all flat-shaded.
//!
//! **Flat-shaded, always.** Every generator here emits one vertex per face
//! corner rather than sharing corners between faces, because a device has no
//! per-face storage — a normal is a vertex attribute or it is nothing — and
//! because the faceted look is what these are for. A caller that wants smooth
//! normals is generating a different mesh, not post-processing one of these.
//!
//! Every one of them fills the box its [`Mesh::scale`] claims: the extremes sit
//! at ±[`Vertex::FULL`] on at least one axis, so none of the sixteen bits a
//! position component has is spent on empty space.

use alloc::vec::Vec;

use corvid_fixed::{Angle32, I16F16, Signed32};
use corvid_vector::{Direction, OctDirection};

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

/// The bit pattern of one in a [`Signed32`], which is what a sine or a cosine
/// comes back at.
const UNIT: i64 = i32::MAX as i64;

/// A cube `half` metres from its centre to each face, flat-shaded, wound
/// counter-clockwise seen from outside.
///
/// Twenty-four vertices rather than eight, because a face's normal belongs to
/// the face: a shared corner would have to average the three faces that meet
/// there, and the whole look is that it does not. Each face is built from a
/// tangent and a bitangent whose cross product is the outward normal, so the
/// winding is right by construction rather than by six copied index lists.
///
/// Every corner sits at ±[`Vertex::FULL`], and `half` is the mesh's scale —
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
    /// order, with `tangent × bitangent = normal`.
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
        let facing = OctDirection::encode(Direction::new(
            Signed32::from_f64(f64::from(normal[0])),
            Signed32::from_f64(f64::from(normal[1])),
            Signed32::from_f64(f64::from(normal[2])),
        ));
        let corner = |along: i32, across: i32| {
            let mut position = [0i16; 3];
            for (axis, component) in position.iter_mut().enumerate() {
                // Every corner of a cube is at full deflection on all three
                // axes, so the sum below is always ±1 and the conversion never
                // saturates.
                *component = i16::try_from(
                    (normal[axis] + along * tangent[axis] + across * bitangent[axis])
                        * i32::from(Vertex::FULL),
                )
                .unwrap_or(Vertex::FULL);
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
/// sit at ±[`Vertex::FULL`] in `x` and `y` and at zero in `z`, so it is flat
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
/// The poles are on `±Z`, which is what puts the extremes at
/// ±[`Vertex::FULL`] exactly: the usual golden-ratio orientation has no
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
/// ±[`Vertex::FULL`] and the shorter one is a fraction of it.
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

/// The outward normal of a triangle wound counter-clockwise as seen from
/// outside: the cross product of its two edges out of the first corner,
/// encoded.
///
/// The cross product is in `i64` because two edges of a full-scale mesh reach
/// 65534 apiece and their product does not fit thirty-two bits. A degenerate
/// triangle has no plane and answers [`OctDirection::UP`], which is what a
/// zeroed vertex holds anyway.
fn face_normal(first: [i16; 3], second: [i16; 3], third: [i16; 3]) -> OctDirection {
    let edge = |from: [i16; 3], to: [i16; 3]| {
        [
            i64::from(to[0]) - i64::from(from[0]),
            i64::from(to[1]) - i64::from(from[1]),
            i64::from(to[2]) - i64::from(from[2]),
        ]
    };
    let (along, across) = (edge(first, second), edge(first, third));
    packed([
        along[1] * across[2] - along[2] * across[1],
        along[2] * across[0] - along[0] * across[2],
        along[0] * across[1] - along[1] * across[0],
    ])
}

/// Three components of any scale, encoded as the direction they point in.
///
/// [`OctDirection::encode`] takes a [`Direction`], whose components are
/// [`Signed32`], and only the ratios between them matter — so the work here is
/// bringing the widest of the three inside thirty-two bits without changing
/// those ratios.
fn packed(components: [i64; 3]) -> OctDirection {
    let widest = components
        .iter()
        .map(|component| component.unsigned_abs())
        .max()
        .unwrap_or(0);
    if widest == 0 {
        return OctDirection::UP;
    }
    let down = corvid_bits::bit_length_u64(widest).saturating_sub(31);
    let component = |index: usize| {
        Signed32::from_bits(i32::try_from(components[index] >> down).unwrap_or(i32::MAX))
    };
    OctDirection::encode(Direction::new(component(0), component(1), component(2)))
}

/// The `step`th of `cells` divisions of `[-FULL, FULL]`.
///
/// Exact at both ends, which is what puts a grid's outer edge on the box its
/// scale claims rather than a division's worth inside it.
fn division(step: u32, cells: u32) -> i16 {
    let reach = i64::from(Vertex::FULL);
    let value = 2 * reach * i64::from(step) / i64::from(cells) - reach;
    i16::try_from(value).unwrap_or(Vertex::FULL)
}

/// The larger of two measurements, which is a mesh's scale when it has two.
fn larger(one: I16F16, other: I16F16) -> I16F16 {
    if other > one { other } else { one }
}

/// `part` as a position component, given that `whole` is what a full one means.
///
/// Zero for a whole that is not positive, which is the degenerate mesh a
/// non-positive size asks for rather than a division by zero.
fn fraction(part: I16F16, whole: I16F16) -> i16 {
    if whole <= I16F16::ZERO {
        return 0;
    }
    let numerator = i64::from(part.to_bits()) * i64::from(Vertex::FULL);
    let denominator = i64::from(whole.to_bits());
    i16::try_from(numerator / denominator).unwrap_or(Vertex::FULL)
}

/// `sides` points evenly around a circle of radius `across`, starting at `+X`.
fn circle(sides: u32, across: i16) -> Vec<[i16; 2]> {
    (0..sides)
        .map(|step| {
            let turn = Angle32::from_bits(wrapped((u64::from(step) << 32) / u64::from(sides)));
            let (sine, cosine) = turn.sin_cos();
            [reach(cosine, across), reach(sine, across)]
        })
        .collect()
}

/// A sine or a cosine, as a position component `across` from the axis.
fn reach(component: Signed32, across: i16) -> i16 {
    let numerator = i64::from(component.to_bits()) * i64::from(across);
    let rounded = round(numerator, UNIT);
    i16::try_from(rounded).unwrap_or(across)
}

/// A unit direction, as a position component `radius` from the origin.
fn on_sphere(direction: Direction, radius: i16) -> [i16; 3] {
    let components = direction.to_array();
    [
        reach(components[0], radius),
        reach(components[1], radius),
        reach(components[2], radius),
    ]
}

/// The unit direction halfway between two, which is what subdividing an edge
/// of a sphere means.
///
/// The midpoint is taken on the bit patterns, where the average of two
/// components is always representable — the *sum* is not, which is why it is
/// formed in `i64` — and then normalized back onto the sphere. Antipodal
/// directions have no midpoint and answer the first of the two, which no edge
/// of an icosahedron is.
fn halfway(one: Direction, other: Direction) -> Direction {
    let (a, b) = (one.to_array(), other.to_array());
    let middle = |index: usize| {
        let sum = i64::from(a[index].to_bits()) + i64::from(b[index].to_bits());
        Signed32::from_bits(i32::try_from(sum / 2).unwrap_or(i32::MAX))
    };
    Direction::new(middle(0), middle(1), middle(2))
        .normalize()
        .unwrap_or(one)
}

/// The twenty faces of an icosahedron with its poles on `±Z`, each wound
/// counter-clockwise seen from outside.
fn icosahedron() -> Vec<[Direction; 3]> {
    /// How many vertices there are in each of the two rings.
    const RING: u32 = 5;

    // The two rings sit at `z = ±1/√5` with radius `2/√5`, so a vertex is
    // `(2cosθ, 2sinθ, ±1)` normalized — which is why the ratio is written out
    // rather than the two irrational components.
    let ring = |offset: u64, up: bool| -> Vec<Direction> {
        (0..RING)
            .map(|step| {
                let turn = Angle32::from_bits(wrapped(
                    ((u64::from(step) * 2 + offset) << 32) / (u64::from(RING) * 2),
                ));
                let (sine, cosine) = turn.sin_cos();
                let pole = if up { UNIT } else { -UNIT };
                unit([
                    2 * i64::from(cosine.to_bits()),
                    2 * i64::from(sine.to_bits()),
                    pole,
                ])
            })
            .collect()
    };
    let upper = ring(0, true);
    let lower = ring(1, false);
    let top = Direction::new(Signed32::ZERO, Signed32::ZERO, Signed32::MAX);
    let bottom = Direction::new(Signed32::ZERO, Signed32::ZERO, Signed32::MIN);

    let mut faces = Vec::with_capacity(20);
    for step in 0..RING as usize {
        let next = (step + 1) % RING as usize;
        faces.push([top, upper[step], upper[next]]);
        faces.push([upper[step], lower[step], upper[next]]);
        faces.push([lower[step], lower[next], upper[next]]);
        faces.push([bottom, lower[next], lower[step]]);
    }
    faces
}

/// Three components of any scale, as the unit direction they point in.
///
/// [`packed`]'s companion: the same rescaling, stopping at the direction rather
/// than going on to the two bytes a vertex stores.
fn unit(components: [i64; 3]) -> Direction {
    let widest = components
        .iter()
        .map(|component| component.unsigned_abs())
        .max()
        .unwrap_or(0);
    let down = corvid_bits::bit_length_u64(widest).saturating_sub(31);
    let component = |index: usize| {
        Signed32::from_bits(i32::try_from(components[index] >> down).unwrap_or(i32::MAX))
    };
    let raw = Direction::new(component(0), component(1), component(2));
    raw.normalize().unwrap_or(Direction::Z)
}

/// A fraction of a turn as the bit pattern an [`Angle32`] wraps at.
///
/// The quotients above are all strictly inside one turn, so the fallback is a
/// spelling of "unreachable" that costs nothing rather than a branch anything
/// takes.
fn wrapped(turns: u64) -> u32 {
    u32::try_from(turns).unwrap_or(0)
}

/// `numerator / denominator`, rounded half away from zero.
const fn round(numerator: i64, denominator: i64) -> i64 {
    let half = denominator / 2;
    if numerator < 0 {
        (numerator - half) / denominator
    } else {
        (numerator + half) / denominator
    }
}
