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

use corvid_fixed::{Angle32, I16F16, Signed16, Signed32};
use corvid_vector::{Direction, OctDirection};

/// The pole component of an icosahedron ring vertex, against a radius of two.
///
/// A ratio rather than a value: `from_ratio` cares only about the proportions,
/// so this is the `1` of `(2 cos, 2 sin, +/-1)` at the same scale the sine and
/// the cosine come back at.
const POLE: i64 = i32::MAX as i64;

/// A position component is a [`Signed16`] bit pattern.
///
/// [`Vertex::FULL`](crate::Vertex::FULL) is `i16::MAX`, so a component at full scale and a
/// [`Signed16`] at `1.0` are the same number. That is what lets every scaling
/// below be `corvid_fixed`'s arithmetic on a normalized value rather than this
/// crate's own on a raw integer.
const fn component(value: Signed16) -> i16 {
    value.to_bits()
}

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
    // `2 * step - cells` over `cells` runs `-1 ..= 1` as `step` runs `0 ..=
    // cells`, and the ends are exact because a whole span saturates rather than
    // rounding.
    component(Signed16::saturating_from_ratio(
        2 * i64::from(step) - i64::from(cells),
        i64::from(cells),
    ))
}

/// `part` as a position component, given that `whole` is what a full one means.
///
/// Zero for a whole that is not positive, which is the degenerate mesh a
/// non-positive size asks for rather than a division by zero.
pub(crate) fn fraction(part: I16F16, whole: I16F16) -> i16 {
    if whole <= I16F16::ZERO {
        return 0;
    }
    component(Signed16::saturating_from_ratio(
        i64::from(part.to_bits()),
        i64::from(whole.to_bits()),
    ))
}

/// `sides` points evenly around a circle of radius `across`, starting at `+X`.
pub(crate) fn circle(sides: u32, across: i16) -> Vec<[i16; 2]> {
    (0..sides)
        .map(|step| {
            let turn = Angle32::from_steps(step, sides);
            let (sine, cosine) = turn.sin_cos();
            [reach(cosine, across), reach(sine, across)]
        })
        .collect()
}

/// A sine or a cosine, as a position component `across` from the axis.
///
/// Both operands denote a fraction of full scale, so this is one multiply of
/// two normalized values rather than a scaling this crate works out for itself.
pub(crate) fn reach(factor: Signed32, across: i16) -> i16 {
    component(Signed16::from_bits(across).saturating_mul(factor.to_signed16()))
}

/// A unit direction, as a position component `radius` from the origin.
pub(crate) fn on_sphere(direction: Direction, radius: i16) -> [i16; 3] {
    direction.to_array().map(|axis| reach(axis, radius))
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
    let sum = |index: usize| i64::from(a[index].to_bits()) + i64::from(b[index].to_bits());

    // The sum rather than the average, because only the ratios reach
    // [`Direction::from_ratio`] and halving them would cost a bit for nothing.
    unit([sum(0), sum(1), sum(2)])
}

/// The twenty faces of an icosahedron with its poles on `+/-Z`, each wound
/// counter-clockwise seen from outside.
pub(crate) fn icosahedron() -> Vec<[Direction; 3]> {
    /// How many vertices there are in each of the two rings.
    const RING: u32 = 5;

    // The two rings sit at `z = +/-1/sqrt5` with radius `2/sqrt5`, so a vertex is
    // `(2costheta, 2sintheta, +/-1)` normalized -- which is why the ratio is written out
    // rather than the two irrational components.
    let ring = |offset: u32, up: bool| -> Vec<Direction> {
        (0..RING)
            .map(|step| {
                let turn = Angle32::from_steps(step * 2 + offset, RING * 2);
                let (sine, cosine) = turn.sin_cos();
                let pole = if up { POLE } else { -POLE };
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
