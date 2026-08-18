//! Where somebody is and where they are going, in six bytes.

use corvid_fixed::I16F16;
use corvid_macros::id_type;
use corvid_vector::FinePoint;

id_type! {
    /// Which triangle of a [`NavMesh`](crate::NavMesh).
    ///
    /// An index into [`NavMesh::tris`](crate::NavMesh::tris), and therefore
    /// also an index into whatever index-parallel arrays the caller keeps
    /// beside them. A million triangles fit; four billion do not fit in memory.
    NavTriRef,
    u32,
    "The triangle's index in its mesh.",
}

/// The tallest a position can be above the triangle it is on, in metres.
///
/// The height code spans this, so it is also what sets the height's resolution:
/// eight metres over 65535 codes is 0.12 mm, and unlike the barycentric codes
/// it does not vary with the triangle, because a height is measured in metres
/// rather than across a face. A body thrown higher than this is
/// not on the surface any more and belongs to whatever the caller uses for
/// things in the air.
pub const MAX_HEIGHT: I16F16 = I16F16::from_bits(8 << 16);

/// The number of fractional bits a barycentric rate code is shifted by.
///
/// One code is `1/16384` of a triangle per second, so the sixteen-bit code
/// reaches +/-2 triangles per second. Both ends are relative to the triangle,
/// because a barycentric rate is: on an eight-metre face that is +/-16 m/s in
/// steps of 0.5 mm/s, on a hundred-metre one +/-200 m/s in steps of 6 mm/s, and
/// at [`MAX_EDGE`](crate::MAX_EDGE) +/-8 km/s in steps of 25 cm/s. The range a
/// walker needs is at the fine end and the quantum a walker needs is at the
/// coarse end, which is why sixteen bits and not eight: eight would have made a
/// walking pace round to nothing on anything larger than a courtyard.
const BARY_RATE_SHIFT: u32 = 2;

/// The number of fractional bits a height rate code is shifted by.
///
/// One code is `1/2048` m/s, so the code reaches +/-16 m/s -- a fall of 13 m,
/// which is further than anything on this surface falls without leaving it.
/// Unlike the barycentric rates this one is in metres, because a height is, so
/// nothing about it varies with the triangle.
const HEIGHT_RATE_SHIFT: u32 = 5;

/// A position and a velocity, decoded into the fine local frame the arithmetic
/// happens in.
///
/// The frame is the one [crs.md] names: `x` and `y` are the barycentric weights
/// of the triangle's first two vertices, and `z` is metres of height along the
/// geocentric up. A velocity is in the same axes per second, so `z` is metres
/// per second and `x` and `y` are triangles per second.
///
/// [crs.md]: https://github.com/peasanttide/peasanttide/blob/main/design/crs.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct NavState {
    /// Which triangle these coordinates are in.
    pub tri: NavTriRef,
    /// Barycentric `x`, barycentric `y`, and height in metres.
    pub position: FinePoint,
    /// The same three axes, per second.
    pub velocity: FinePoint,
}

/// A position on the surface, and a velocity along it.
///
/// Sixteen bytes, of which the twelve that scale are
/// [`position`](Self::position) and [`velocity`](Self::velocity): a crowd
/// stores its agents grouped by triangle, so [`tri`](Self::tri) is paid once
/// per bucket rather than once per agent. [`local_bytes`](Self::local_bytes) is
/// that twelve-byte half on its own.
///
/// The coordinates are barycentric, so **a `NavCords` is on the surface by
/// construction**. There is no "is this agent standing on the ground" to ask,
/// because there is no way to write down an agent that is not.
///
/// What one code is worth in metres is a fact about the triangle rather than
/// about the coordinate: sixteen bits across a face spans its longest edge, so
/// [`NavTri::resolution`](crate::NavTri::resolution) is where a caller asks
/// what its own level gives. Eight metres over 65535 codes is 0.12 mm and
/// [`MAX_EDGE`](crate::MAX_EDGE) over the same is 6.3 cm.
///
/// ```
/// use corvid_nav::{NavCords, NavTriRef};
///
/// let stood = NavCords::centred(NavTriRef(3));
/// assert_eq!(stood.local_bytes(), [85, 85, 85, 85, 0, 0, 0, 0, 0, 0, 0, 0]);
/// assert!(stood.is_inside());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NavCords {
    /// Which triangle of the mesh.
    pub tri: NavTriRef,
    /// Barycentric `x`, barycentric `y`, and height as a fraction of
    /// [`MAX_HEIGHT`], each as a `UNORM` code so that `0` and `65535` are
    /// exactly the two ends.
    pub position: [u16; 3],
    /// The velocity along the same three axes, signed and symmetric: `-32768`
    /// never occurs, so negating a velocity is exact.
    pub velocity: [i16; 3],
}

impl NavCords {
    /// A resting position at the given barycentric and height codes.
    #[must_use]
    #[inline]
    pub const fn at(tri: NavTriRef, position: [u16; 3]) -> Self {
        Self {
            tri,
            position,
            velocity: [0; 3],
        }
    }

    /// A resting position in the middle of a triangle, on the ground.
    ///
    /// The centroid is a third of each weight, which no code divides exactly,
    /// so this is the nearest pair that does not favour one vertex over
    /// another. What it is for is a caller who wants somebody standing in a
    /// named triangle and does not care where.
    #[must_use]
    #[inline]
    pub const fn centred(tri: NavTriRef) -> Self {
        Self::at(tri, [21_845, 21_845, 0])
    }

    /// The twelve bytes that a crowd stores per agent.
    ///
    /// Position first, then velocity, which is the order the fields are
    /// declared in and the order the decoder reads them, each in little-endian
    /// order for the reason everything else on the wire is.
    #[must_use]
    #[inline]
    pub const fn local_bytes(self) -> [u8; 12] {
        let [x, y, height] = self.position;
        let [dx, dy, rise] = self.velocity;
        let [x, y, height] = [x.to_le_bytes(), y.to_le_bytes(), height.to_le_bytes()];
        let [dx, dy, rise] = [dx.to_le_bytes(), dy.to_le_bytes(), rise.to_le_bytes()];
        [
            x[0], x[1], y[0], y[1], height[0], height[1], dx[0], dx[1], dy[0], dy[1], rise[0],
            rise[1],
        ]
    }

    /// Whether the two barycentric codes lie inside the triangle.
    ///
    /// The third weight is `65535 - x - y`, so the whole of the condition is
    /// that the two codes sum to at most 65535. [`encode`](Self::encode)
    /// guarantees it, and this is how a caller checks a `NavCords` it built by
    /// hand.
    #[must_use]
    #[inline]
    pub const fn is_inside(self) -> bool {
        self.position[0] as u32 + self.position[1] as u32 <= 65_535
    }

    /// The fine local frame these bytes denote.
    ///
    /// Exact in both directions for the position: every code round-trips
    /// through [`encode`](Self::encode) unchanged, which is what makes a walk
    /// across a seam land where arithmetic says rather than a step downhill of
    /// it.
    #[must_use]
    #[inline]
    pub const fn decode(self) -> NavState {
        NavState {
            tri: self.tri,
            position: FinePoint::new(
                from_unorm16(self.position[0]),
                from_unorm16(self.position[1]),
                height_from_code(self.position[2]),
            ),
            velocity: FinePoint::new(
                rate_from_code(self.velocity[0], BARY_RATE_SHIFT),
                rate_from_code(self.velocity[1], BARY_RATE_SHIFT),
                rate_from_code(self.velocity[2], HEIGHT_RATE_SHIFT),
            ),
        }
    }

    /// The bytes for a fine local state.
    ///
    /// Rounds to nearest, then repairs the one thing rounding can break: two
    /// barycentric codes that sum past 65535 would name a point outside the
    /// triangle, so the larger of the two gives up the last codes. That keeps
    /// the invariant [`is_inside`](Self::is_inside) states, and it is
    /// deterministic down to which of a tied pair gives way -- `y` does.
    ///
    /// Motion finer than a code is lost, which is 0.12 mm on an eight-metre
    /// triangle and 6.3 cm on one at [`MAX_EDGE`](crate::MAX_EDGE). A caller
    /// whose agents move less than that in a tick wants a longer tick or a
    /// finer triangulation, and [`NavTri::resolution`](crate::NavTri::resolution)
    /// is what tells them which they have.
    #[must_use]
    #[inline]
    pub const fn encode(state: NavState) -> Self {
        let mut x = to_unorm16(state.position.x());
        let mut y = to_unorm16(state.position.y());
        let over = x as u32 + y as u32;
        if over > 65_535 {
            let excess = (over - 65_535) as u16;
            if y >= x {
                y -= excess;
            } else {
                x -= excess;
            }
        }
        Self {
            tri: state.tri,
            position: [x, y, height_to_code(state.position.z())],
            velocity: [
                rate_to_code(state.velocity.x(), BARY_RATE_SHIFT),
                rate_to_code(state.velocity.y(), BARY_RATE_SHIFT),
                rate_to_code(state.velocity.z(), HEIGHT_RATE_SHIFT),
            ],
        }
    }
}

/// The fraction a sixteen-bit code denotes, `code / 65535`.
///
/// The `UNORM` convention [`I16F16::from_unorm8`] states, one width up: 0 is
/// none of it, 65535 is all of it, and the step is `1/65535` so that both ends
/// are exact. [`to_unorm16`] is its inverse for every one of the 65536 codes.
#[inline]
const fn from_unorm16(code: u16) -> I16F16 {
    let numerator = (code as i64) << 16;
    I16F16::saturating_from_bits((numerator + 32_767) / 65_535)
}

/// The sixteen-bit code a fraction denotes, rounded and clamped.
#[inline]
const fn to_unorm16(value: I16F16) -> u16 {
    let scaled = ((value.to_bits() as i64) * 65_535 + 32_768) >> 16;
    if scaled <= 0 {
        0
    } else if scaled >= 65_535 {
        65_535
    } else {
        scaled as u16
    }
}

/// The height a code denotes, in metres.
#[inline]
const fn height_from_code(code: u16) -> I16F16 {
    I16F16::from_bits(from_unorm16(code).to_bits() * 8)
}

/// The code for a height in metres, rounded and clamped to `0 ..= MAX_HEIGHT`.
///
/// Clamping at zero is where "never underground" is finally enforced: whatever
/// the arithmetic did, what gets written down is on or above the surface.
#[inline]
const fn height_to_code(height: I16F16) -> u16 {
    if height.to_bits() <= 0 {
        return 0;
    }
    to_unorm16(I16F16::from_bits(height.to_bits() / 8))
}

/// The rate a code denotes, at the given quantum.
#[inline]
const fn rate_from_code(code: i16, shift: u32) -> I16F16 {
    I16F16::from_bits((code as i32) << shift)
}

/// The code for a rate, rounded away from zero at the halfway point and clamped
/// symmetrically.
#[inline]
const fn rate_to_code(rate: I16F16, shift: u32) -> i16 {
    let half = 1i64 << (shift - 1);
    let value = rate.to_bits() as i64;
    let rounded = if value >= 0 {
        (value + half) >> shift
    } else {
        -((-value + half) >> shift)
    };
    if rounded > 32_767 {
        32_767
    } else if rounded < -32_767 {
        -32_767
    } else {
        rounded as i16
    }
}
