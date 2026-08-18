//! The level's own horizontal, which is what a grid over a city indexes in.

use corvid_vector::{Direction, GlobalPoint};

/// Two horizontal axes and the point they are measured from.
///
/// ECEF is the frame a world position is written in and it is the wrong frame
/// to index a city in: a level plane at Paris's latitude lies across all three
/// of its axes, so a square district is a diagonal slab of any grid built on
/// them and spends its budget in every direction at once. The fix is to measure
/// where a thing is in the plane the level is actually flat in, which is the
/// tangent plane at its own middle.
///
/// East is the earth's polar axis crossed with the local up, north is the up
/// crossed with east, and both are unit. What they answer is two numbers in
/// metres, which is what a grid divides into cells.
///
/// A level directly over a pole has no east in that sense -- the polar axis and
/// its up are the same line -- so the axis crossed with the up is whichever of
/// the three ECEF axes leans least on it. That keeps the pair perpendicular and
/// deterministic everywhere, at the price of the names being a direction rather
/// than a bearing for a level nobody will ever build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NavPlane {
    origin: GlobalPoint,
    east: Direction,
    north: Direction,
}

impl NavPlane {
    /// The plane a box of world positions lies in.
    ///
    /// The origin is the low corner, so a level's own coordinates start at zero
    /// and stay positive; the up is taken at the middle of the box, because
    /// that is where the level is and a tangent plane is only tangent
    /// somewhere. A box at the earth's centre, or one straddling a pole, has no
    /// east to speak of and falls back to the ECEF axes, which is the honest
    /// answer for a level nobody has said where to put.
    #[must_use]
    pub fn over(low: GlobalPoint, high: GlobalPoint) -> Self {
        let middle = [
            i64::from(low.x().to_bits()) + i64::from(high.x().to_bits()),
            i64::from(low.y().to_bits()) + i64::from(high.y().to_bits()),
            i64::from(low.z().to_bits()) + i64::from(high.z().to_bits()),
        ];
        let up = Direction::from_ratio(middle).unwrap_or(Direction::Z);
        let east = cross(sidelong(up), up).unwrap_or(Direction::X);
        let north = cross(up, east).unwrap_or(Direction::Y);
        Self {
            origin: low,
            east,
            north,
        }
    }

    /// The point the two axes are measured from.
    #[must_use]
    #[inline]
    pub const fn origin(self) -> GlobalPoint {
        self.origin
    }

    /// The eastward axis.
    #[must_use]
    #[inline]
    pub const fn east(self) -> Direction {
        self.east
    }

    /// The northward axis.
    #[must_use]
    #[inline]
    pub const fn north(self) -> Direction {
        self.north
    }

    /// How far east and how far north a world position is, in metres.
    ///
    /// Height is not answered because a grid over a city does not want it: what
    /// is above a street is the street's business and not the index's.
    #[must_use]
    #[inline]
    pub fn offsets(self, point: GlobalPoint) -> [i32; 2] {
        let offset = point.sub(self.origin);
        [
            offset.project(self.east).to_bits(),
            offset.project(self.north).to_bits(),
        ]
    }
}

/// The ECEF axis that leans least on a direction.
///
/// The polar axis wherever a city is, and something else only for a level over
/// a pole, where the polar axis would have crossed with the up into nothing.
fn sidelong(up: Direction) -> Direction {
    let mut chosen = Direction::Z;
    let mut leaning = up.align(Direction::Z).to_bits().unsigned_abs();
    for axis in [Direction::X, Direction::Y] {
        let against = up.align(axis).to_bits().unsigned_abs();
        if against < leaning {
            leaning = against;
            chosen = axis;
        }
    }
    chosen
}

/// The unit direction two others cross into, or [`None`] if they are parallel.
///
/// Through [`Direction::from_ratio`] rather than through a component-wise
/// cross product, because only the ratios matter and the products of two Q31
/// patterns want a word.
fn cross(a: Direction, b: Direction) -> Option<Direction> {
    let [ax, ay, az] = a
        .to_array()
        .map(|value| i128::from(value.canonicalize().to_bits()));
    let [bx, by, bz] = b
        .to_array()
        .map(|value| i128::from(value.canonicalize().to_bits()));
    // Two Q31 patterns multiply into Q62 and two of those subtract into one
    // bit more than a word holds, so the answer comes down two bits before it
    // is handed over. Only the ratios matter, and they are unchanged.
    let narrow = |value: i128| (value >> 2) as i64;
    Direction::from_ratio([
        narrow(ay * bz - az * by),
        narrow(az * bx - ax * bz),
        narrow(ax * by - ay * bx),
    ])
}

impl Default for NavPlane {
    #[inline]
    fn default() -> Self {
        Self {
            origin: GlobalPoint::ZERO,
            east: Direction::X,
            north: Direction::Y,
        }
    }
}
