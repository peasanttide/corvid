//! The arithmetic the generators are written in terms of.
//!
//! Split from [`shapes`](crate::shapes) because the two answer different
//! questions: that module says what a cube is, and this one says how a normal
//! is derived, how a circle is stepped round and where a point on a sphere
//! lands. Nothing here knows what shape it is serving.
//!
//! None of it is hashed. A mesh is client-ring -- it is what a device draws,
//! not something a peer agrees with -- so the roundings here are chosen to put
//! the extremes exactly on the box a mesh claims rather than to be reproducible
//! against another machine's.

use alloc::vec::Vec;

use corvid_fixed::{Angle32, I16F16, Signed32};
use corvid_vector::{Direction, OctDirection};

use crate::Vertex;

/// The bit pattern of one in a [`Signed32`], which is what a sine or a cosine
/// comes back at.
const UNIT: i64 = i32::MAX as i64;

/// The outward normal of a triangle wound counter-clockwise as seen from
/// outside: the cross product of its two edges out of the first corner,
/// encoded.
///
/// The cross product is in `i64` because two edges of a full-scale mesh reach
/// 65534 apiece and their product does not fit thirty-two bits. A degenerate
/// triangle has no plane and answers [`OctDirection::UP`], which is what a
/// zeroed vertex holds anyway.
pub(crate) fn face_normal(first: [i16; 3], second: [i16; 3], third: [i16; 3]) -> OctDirection {
    let edge = |from: [i16; 3], to: [i16; 3]| {
        [
            i64::from(to[0]) - i64::from(from[0]),
            i64::from(to[1]) - i64::from(from[1]),
            i64::from(to[2]) - i64::from(from[2]),
        ]
    };
    let (along, across) = (edge(first, second), edge(first, third));

    // `from_ratio` rather than a shift and a `Direction::new` here: only the
    // ratios matter to the octahedral map, but a `Direction` is a *unit* vector
    // by construction, and building one out of rescaled components would put a
    // value in the type that is not one. The rescale this crate was doing is
    // the first step of `from_ratio` anyway.
    Direction::from_ratio([
        along[1] * across[2] - along[2] * across[1],
        along[2] * across[0] - along[0] * across[2],
        along[0] * across[1] - along[1] * across[0],
    ])
    .map_or(OctDirection::UP, OctDirection::encode)
}

/// The `step`th of `cells` divisions of `[-FULL, FULL]`.
///
/// Exact at both ends, which is what puts a grid's outer edge on the box its
/// scale claims rather than a division's worth inside it.
pub(crate) fn division(step: u32, cells: u32) -> i16 {
    let reach = i64::from(Vertex::FULL);
    let value = 2 * reach * i64::from(step) / i64::from(cells) - reach;
    i16::try_from(value).unwrap_or(Vertex::FULL)
}

/// The larger of two measurements, which is a mesh's scale when it has two.
pub(crate) fn larger(one: I16F16, other: I16F16) -> I16F16 {
    if other > one { other } else { one }
}

/// `part` as a position component, given that `whole` is what a full one means.
///
/// Zero for a whole that is not positive, which is the degenerate mesh a
/// non-positive size asks for rather than a division by zero.
pub(crate) fn fraction(part: I16F16, whole: I16F16) -> i16 {
    if whole <= I16F16::ZERO {
        return 0;
    }
    let numerator = i64::from(part.to_bits()) * i64::from(Vertex::FULL);
    let denominator = i64::from(whole.to_bits());
    i16::try_from(numerator / denominator).unwrap_or(Vertex::FULL)
}

/// `sides` points evenly around a circle of radius `across`, starting at `+X`.
pub(crate) fn circle(sides: u32, across: i16) -> Vec<[i16; 2]> {
    (0..sides)
        .map(|step| {
            let turn = Angle32::from_bits(wrapped((u64::from(step) << 32) / u64::from(sides)));
            let (sine, cosine) = turn.sin_cos();
            [reach(cosine, across), reach(sine, across)]
        })
        .collect()
}

/// A sine or a cosine, as a position component `across` from the axis.
pub(crate) fn reach(component: Signed32, across: i16) -> i16 {
    let numerator = i64::from(component.to_bits()) * i64::from(across);
    let rounded = round(numerator, UNIT);
    i16::try_from(rounded).unwrap_or(across)
}

/// A unit direction, as a position component `radius` from the origin.
pub(crate) fn on_sphere(direction: Direction, radius: i16) -> [i16; 3] {
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
/// components is always representable -- the *sum* is not, which is why it is
/// formed in `i64` -- and then normalized back onto the sphere. Antipodal
/// directions have no midpoint and answer the first of the two, which no edge
/// of an icosahedron is.
pub(crate) fn halfway(one: Direction, other: Direction) -> Direction {
    let (a, b) = (one.to_array(), other.to_array());
    let middle = |index: usize| {
        let sum = i64::from(a[index].to_bits()) + i64::from(b[index].to_bits());
        Signed32::from_bits(i32::try_from(sum / 2).unwrap_or(i32::MAX))
    };
    Direction::new(middle(0), middle(1), middle(2))
        .normalize()
        .unwrap_or(one)
}

/// The twenty faces of an icosahedron with its poles on `+/-Z`, each wound
/// counter-clockwise seen from outside.
pub(crate) fn icosahedron() -> Vec<[Direction; 3]> {
    /// How many vertices there are in each of the two rings.
    const RING: u32 = 5;

    // The two rings sit at `z = +/-1/sqrt5` with radius `2/sqrt5`, so a vertex is
    // `(2costheta, 2sintheta, +/-1)` normalized -- which is why the ratio is written out
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
/// [`Direction::from_ratio`] with a name for the one case it cannot answer.
/// Every caller here is a vertex of an icosahedron or a midpoint between two of
/// them, so none of them is the zero vector -- but the constructor cannot know
/// that, and `Z` is a better answer to an impossible input than a panic.
pub(crate) fn unit(components: [i64; 3]) -> Direction {
    Direction::from_ratio(components).unwrap_or(Direction::Z)
}

/// A fraction of a turn as the bit pattern an [`Angle32`] wraps at.
///
/// The quotients above are all strictly inside one turn, so the fallback is a
/// spelling of "unreachable" that costs nothing rather than a branch anything
/// takes.
pub(crate) fn wrapped(turns: u64) -> u32 {
    u32::try_from(turns).unwrap_or(0)
}

/// `numerator / denominator`, rounded half away from zero.
pub(crate) const fn round(numerator: i64, denominator: i64) -> i64 {
    let half = denominator / 2;
    if numerator < 0 {
        (numerator - half) / denominator
    } else {
        (numerator + half) / denominator
    }
}
