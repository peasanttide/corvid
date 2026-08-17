//! How one particle moves: a constant acceleration and a linear drag.

use corvid_glm::Vec3;

/// One step of `dv/dt = gravity - drag * v`, with the drag taken implicitly.
///
/// The scheme is
///
/// ```text
/// v <- (v + gravity * dt) / (1 + drag * dt)
/// x <- x + v * dt
/// ```
///
/// which is unconditionally stable: the divisor is never below one, so a
/// velocity can only shrink toward the terminal velocity however large the step
/// or the drag. The explicit form `v <- v * (1 - drag * dt) + gravity * dt`
/// looks cheaper and oscillates once `drag * dt` passes one and diverges once it
/// passes two -- and an ember in still air is a drag of two per second, so a
/// display running at four frames a second would fling it into the sky.
///
/// It is also exactly composable, which is what makes the motion testable
/// against arithmetic rather than against a previous run. Writing
/// `terminal = gravity / drag` and `decay = 1 / (1 + drag * dt)`, `n` steps from
/// `x0`, `v0` give
///
/// ```text
/// v(n) = terminal + (v0 - terminal) * decay^n
/// x(n) = x0 + terminal * n * dt + (v0 - terminal) * (1 - decay^n) / drag
/// ```
///
/// and with no drag at all the same steps are the exact ballistic
/// `v(n) = v0 + gravity * n * dt`, `x(n) = x0 + v0 * n * dt + gravity * dt^2 *
/// n * (n + 1) / 2` -- the half-step offset being the price of taking the
/// end-of-step velocity, which is the price stability costs. `tests/motion.rs`
/// holds both against a hundred steps.
pub(crate) fn advance(
    position: Vec3,
    velocity: Vec3,
    gravity: Vec3,
    drag: f32,
    dt: f32,
) -> (Vec3, Vec3) {
    let accelerated = velocity + gravity * dt;
    let moved = if drag > 0.0 {
        accelerated / (1.0 + drag * dt)
    } else {
        accelerated
    };
    (position + moved * dt, moved)
}
