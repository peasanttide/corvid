//! The two vector operations that need a square root.
//!
//! Written here rather than taken from `nalgebra` so that every root in this
//! crate is [`corvid_float::sqrt`]. A `no_std` `nalgebra` takes its root from
//! `libm` and a hosted one takes it from the hardware, and a crate whose whole
//! promise is that a seed repeats cannot have two answers for one seed.

use corvid_glm::Vec3;

/// The unit vector along `v`, or `fallback` when `v` has no direction.
///
/// The fallback is the caller's rather than a constant, because the sensible
/// answer differs: a sphere sampled exactly at its centre has no outward
/// direction and wants a random one, and a cone around a zero axis wants the
/// world's up.
pub(crate) fn normalized(v: Vec3, fallback: Vec3) -> Vec3 {
    let square = v.x * v.x + v.y * v.y + v.z * v.z;
    // Not `> 0.0`: a vector a micrometre long normalizes to something whose
    // components are 1e6 times its own rounding error, and the direction that
    // comes out is noise rather than a direction. Below this the caller's
    // fallback is the more honest answer.
    if square > 1e-12 {
        v / corvid_float::sqrt(square)
    } else {
        fallback
    }
}

/// An orthonormal basis whose third vector is the unit vector along `axis`.
///
/// The first two are perpendicular to it and to each other, in no particular
/// roll -- what uses them samples a full turn around the axis, so where the turn
/// starts cannot be observed. What can be observed is that the basis is a
/// function of the axis alone, so two runs with one seed build the same one.
pub(crate) fn basis(axis: Vec3) -> (Vec3, Vec3, Vec3) {
    let forward = normalized(axis, Vec3::new(0.0, 0.0, 1.0));
    // The world axis the direction is least aligned with, so that the cross
    // product below is never near zero and the basis never degenerates.
    let seed = if corvid_float::abs(forward.z) < 0.9 {
        Vec3::new(0.0, 0.0, 1.0)
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    let right = normalized(forward.cross(&seed), Vec3::new(1.0, 0.0, 0.0));
    let up = forward.cross(&right);
    (right, up, forward)
}
