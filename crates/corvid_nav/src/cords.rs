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
/// The height byte spans this, so it is also what sets the height's resolution:
/// eight metres over 255 codes is 3.1 cm, matching what the barycentric bytes
/// give across a triangle of the same size. A body thrown higher than this is
/// not on the surface any more and belongs to whatever the caller uses for
/// things in the air.
pub const MAX_HEIGHT: I16F16 = I16F16::from_bits(8 << 16);

/// The number of fractional bits a barycentric rate byte is shifted by.
///
/// One code is `1/64` of a triangle per second, so the byte reaches +/-1.98
/// triangles per second: on an eight-metre triangle, +/-15.9 m/s at 12.5 cm/s
/// steps, and on a two-metre one +/-4 m/s at 3.1 cm/s. Fine triangulation is
/// where fine motion is wanted, so tying the quantum to the triangle is the
/// right way round.
const BARY_RATE_SHIFT: u32 = 10;

/// The number of fractional bits a height rate byte is shifted by.
///
/// One code is `1/8` m/s, so the byte reaches +/-15.875 m/s -- a fall of 12.8 m,
/// which is further than anything on this surface falls without leaving it.
/// Unlike the barycentric rates this one is in metres, because a height is.
const HEIGHT_RATE_SHIFT: u32 = 13;

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
/// Ten bytes, of which the six that scale are [`position`](Self::position) and
/// [`velocity`](Self::velocity): a crowd stores its agents grouped by triangle,
/// so [`tri`](Self::tri) is paid once per bucket rather than once per agent.
/// [`local_bytes`](Self::local_bytes) is that six-byte half on its own.
///
/// The coordinates are barycentric, so **a `NavCords` is on the surface by
/// construction**. There is no "is this agent standing on the ground" to ask,
/// because there is no way to write down an agent that is not.
///
/// ```
/// use corvid_nav::NavCords;
///
/// let stood = NavCords::at(corvid_nav::NavTriRef(3), [85, 85, 0]);
/// assert_eq!(stood.local_bytes(), [85, 85, 0, 0, 0, 0]);
/// assert!(stood.is_inside());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NavCords {
    /// Which triangle of the mesh.
    pub tri: NavTriRef,
    /// Barycentric `x`, barycentric `y`, and height as a fraction of
    /// [`MAX_HEIGHT`], each as a `UNORM` code so that `0` and `255` are exactly
    /// the two ends.
    pub position: [u8; 3],
    /// The velocity along the same three axes, signed and symmetric: `-128`
    /// never occurs, so negating a velocity is exact.
    pub velocity: [i8; 3],
}

impl NavCords {
    /// A resting position at the given barycentric and height codes.
    #[must_use]
    #[inline]
    pub const fn at(tri: NavTriRef, position: [u8; 3]) -> Self {
        Self {
            tri,
            position,
            velocity: [0; 3],
        }
    }

    /// The six bytes that a crowd stores per agent.
    ///
    /// Position first, then velocity, which is the order the fields are
    /// declared in and the order the decoder reads them.
    #[must_use]
    #[inline]
    pub const fn local_bytes(self) -> [u8; 6] {
        [
            self.position[0],
            self.position[1],
            self.position[2],
            self.velocity[0] as u8,
            self.velocity[1] as u8,
            self.velocity[2] as u8,
        ]
    }

    /// Whether the two barycentric codes lie inside the triangle.
    ///
    /// The third weight is `255 - x - y`, so the whole of the condition is that
    /// the two codes sum to at most 255. [`encode`](Self::encode) guarantees
    /// it, and this is how a caller checks a `NavCords` it built by hand.
    #[must_use]
    #[inline]
    pub const fn is_inside(self) -> bool {
        self.position[0] as u16 + self.position[1] as u16 <= 255
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
                I16F16::from_unorm8(self.position[0]),
                I16F16::from_unorm8(self.position[1]),
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
    /// barycentric codes that sum past 255 would name a point outside the
    /// triangle, so the larger of the two gives up the last code. That keeps
    /// the invariant [`is_inside`](Self::is_inside) states, and it is
    /// deterministic down to which of a tied pair gives way -- `y` does.
    ///
    /// Motion finer than a code is lost, which is 3.1 cm on an eight-metre
    /// triangle and is the price of six bytes. A caller whose agents move less
    /// than that per tick wants a longer tick, not a finer coordinate.
    #[must_use]
    #[inline]
    pub const fn encode(state: NavState) -> Self {
        let mut x = state.position.x().to_unorm8();
        let mut y = state.position.y().to_unorm8();
        let over = x as u16 + y as u16;
        if over > 255 {
            let excess = (over - 255) as u8;
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

/// The height a code denotes, in metres.
#[inline]
const fn height_from_code(code: u8) -> I16F16 {
    I16F16::from_bits(I16F16::from_unorm8(code).to_bits() * 8)
}

/// The code for a height in metres, rounded and clamped to `0 ..= MAX_HEIGHT`.
///
/// Clamping at zero is where "never underground" is finally enforced: whatever
/// the arithmetic did, what gets written down is on or above the surface.
#[inline]
const fn height_to_code(height: I16F16) -> u8 {
    if height.to_bits() <= 0 {
        return 0;
    }
    I16F16::from_bits(height.to_bits() / 8).to_unorm8()
}

/// The rate a code denotes, at the given quantum.
#[inline]
const fn rate_from_code(code: i8, shift: u32) -> I16F16 {
    I16F16::from_bits((code as i32) << shift)
}

/// The code for a rate, rounded away from zero at the halfway point and clamped
/// symmetrically.
#[inline]
const fn rate_to_code(rate: I16F16, shift: u32) -> i8 {
    let half = 1i64 << (shift - 1);
    let value = rate.to_bits() as i64;
    let rounded = if value >= 0 {
        (value + half) >> shift
    } else {
        -((-value + half) >> shift)
    };
    if rounded > 127 {
        127
    } else if rounded < -127 {
        -127
    } else {
        rounded as i8
    }
}
