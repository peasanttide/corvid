//! The ballistic substep loop, and the two calculations it picks between.

use corvid_fixed::{Angle16, Factor16, I16F16};
use corvid_vector::{Direction, FinePoint};

use crate::cords::{NavCords, NavState};
use crate::error::NavError;
use crate::mesh::NavMesh;
use crate::tri::{NavTri, fine_direction};

/// The numbers a game gets to choose.
///
/// Everything here is a property of the world rather than of the mesh, so two
/// levels share a mesh format and disagree about gravity. All of it is fixed
/// point, all of it reaches the hashed state, and a peer that tunes differently
/// desyncs on the first bounce -- which is why a `Tune` belongs in the opening
/// a session agrees on and not in a settings file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tune {
    /// Downward acceleration in metres per second squared.
    pub gravity: I16F16,
    /// The fraction of a velocity shed per second of flight.
    pub drag: Factor16,
    /// The fraction of the approach speed kept when a hit bounces.
    pub restitution: Factor16,
    /// The incidence angle, measured from the surface, at or below which a hit
    /// slides instead of bouncing.
    ///
    /// A grazing hit is a shallow angle and slides; a head-on one is a steep
    /// angle and bounces. Stored as an angle and compared as a sine, so no
    /// trigonometry runs in a tick.
    pub slide_angle: Angle16,
    /// How far the two sides of a seam may disagree in height and still be
    /// walkable.
    pub step_height: I16F16,
    /// The steepest face a body may walk *onto*.
    ///
    /// Looser than the limit the local frame itself imposes, which is where
    /// [`NavError::FaceTooSteep`] comes from: this one is what makes a cliff
    /// face unwalkable rather than merely awkward.
    pub max_slope: Angle16,
    /// How many events one call may resolve before it gives up and spends the
    /// rest of the time flying straight.
    ///
    /// A walking agent uses two or three. The cap is what keeps a body wedged
    /// in a corner from turning a tick into a search.
    pub max_events: u8,
}

impl Default for Tune {
    fn default() -> Self {
        Self {
            gravity: I16F16::from_bits(642_693),
            drag: Factor16::from_bits(1310),
            restitution: Factor16::from_bits(16_384),
            slide_angle: Angle16::from_degrees(45.0),
            step_height: I16F16::from_bits(22_938),
            max_slope: Angle16::from_degrees(50.0),
            max_events: 8,
        }
    }
}

/// Something that happens partway through a step.
///
/// [`duration`](Self::duration) is how much of the remaining time it took to
/// get there, and [`state`](Self::state) is where the body is and how it is
/// moving once it has happened -- after the bounce, after the slide, after the
/// seam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavEvent {
    /// How long the body travelled before this happened.
    pub duration: I16F16,
    /// The body's state once it has.
    pub state: NavState,
}

/// The event that happens first, or [`None`] if neither does.
///
/// A tie goes to the earlier entry, which is how the caller orders the two: a
/// body standing still on a seam resolves the ground before the crossing, so it
/// crosses already sliding rather than crossing and then discovering the floor.
#[must_use]
#[inline]
pub fn pick_next_event(events: [Option<NavEvent>; 2]) -> Option<NavEvent> {
    match events {
        [Some(first), Some(second)] => {
            if second.duration < first.duration {
                Some(second)
            } else {
                Some(first)
            }
        }
        [first, None] => first,
        [None, second] => second,
    }
}

/// When and how the body meets the triangle it is on.
///
/// The plane of a triangle is `z == 0` in its own local frame, so this is a
/// sign test and one division rather than a plane equation. A body already
/// moving away, or one that will not reach the plane inside `remaining`, has no
/// event.
///
/// The angle of incidence decides what happens: at or below
/// [`Tune::slide_angle`] the velocity is projected onto the face and the body
/// slides, above it the normal component reverses and keeps
/// [`Tune::restitution`] of itself. Sliding is what makes a ramp accelerate a
/// body downhill -- gravity goes into the plane, the projection takes out the
/// part that would go through it, and what is left points down the slope.
#[must_use]
pub fn calc_collision_vs_plane(
    tri: &NavTri,
    state: NavState,
    remaining: I16F16,
    tune: &Tune,
) -> Option<NavEvent> {
    let height = state.position.z();
    let rate = state.velocity.z();
    if !rate.is_negative() || height.is_negative() {
        return None;
    }
    let when = height.saturating_div(rate.saturating_neg());
    if when > remaining {
        return None;
    }

    // A hit that takes no time and changes no velocity is not an event, it is
    // the same instant over again. Reporting one would spend the whole
    // iteration budget standing still on a face whose normal component rounds
    // to nothing.
    let resolved = resolve_ground(tri, state.velocity, tune);
    if when.is_zero() && resolved == state.velocity {
        return None;
    }

    let travelled = state.position.add(state.velocity.mul(when));
    let landed = NavTri::clamp_inside(FinePoint::new(travelled.x(), travelled.y(), I16F16::ZERO));
    Some(NavEvent {
        duration: when,
        state: NavState {
            tri: state.tri,
            position: landed,
            velocity: resolved,
        },
    })
}

/// When and where the body leaves the triangle it is on.
///
/// The three edges are three linear inequalities in the local frame -- `x >= 0`,
/// `y >= 0` and `x + y <= 1` -- so the crossing time is a division and the
/// crossing point is on the line by construction. A tie between two edges goes
/// to the lower edge index, which is the same deterministic rule as the lower
/// triangle index winning a shared vertex.
///
/// A walkable seam carries the position and the velocity into the neighbour and
/// clamps the arrival above the neighbour's own plane, so the body is never
/// left underground. An unwalkable one, and a boundary edge with no neighbour
/// at all, is a vertical wall standing on the edge: the body bounces off it and
/// stays in the triangle it was in, which is why a walker does not fall off a
/// cliff.
#[must_use]
pub fn calc_next_nav_tri(
    tri: &NavTri,
    state: NavState,
    remaining: I16F16,
    tune: &Tune,
) -> Option<NavEvent> {
    let position = state.position;
    let velocity = state.velocity;
    let one = I16F16::ONE;
    let boundaries = [
        (
            one.saturating_sub(position.x())
                .saturating_sub(position.y()),
            velocity.x().saturating_add(velocity.y()).saturating_neg(),
        ),
        (position.x(), velocity.x()),
        (position.y(), velocity.y()),
    ];

    let mut chosen: Option<(usize, I16F16)> = None;
    let mut index = 0;
    while index < 3 {
        let (distance, rate) = boundaries[index];
        if rate.is_negative() && !distance.is_negative() {
            let when = distance.saturating_div(rate.saturating_neg());
            if when <= remaining && chosen.is_none_or(|(_, best)| when < best) {
                chosen = Some((index, when));
            }
        }
        index += 1;
    }

    let (edge, when) = chosen?;
    let travelled = NavTri::clamp_inside(position.add(velocity.mul(when)));

    match tri.edge(edge) {
        Some(seam) if seam.is_walkable() => Some(NavEvent {
            duration: when,
            state: NavState {
                tri: seam.next(),
                position: NavTri::clamp_inside(seam.local_to_next().apply(travelled)),
                velocity: seam.vel_to_next().apply(velocity),
            },
        }),
        _ => Some(NavEvent {
            duration: when,
            state: NavState {
                tri: state.tri,
                position: travelled,
                velocity: resolve_wall(tri, velocity, edge, tune),
            },
        }),
    }
}

/// Advances one body by `duration` seconds.
///
/// This is the loop [physics.md] specifies, and the order of it is the whole
/// design: compute both candidate events against the *straight line* the body
/// is on, take whichever happens first, advance exactly to it, then apply
/// gravity and drag over the time that took and go round again. Forces are
/// integrated between events rather than through them, so a bounce is resolved
/// against the velocity the body actually arrived with.
///
/// # Errors
///
/// [`NavError::UnknownTriangle`] if the coordinates name a triangle this mesh
/// does not have, which is the one thing a step cannot recover from -- there is
/// no position to fall back to.
///
/// [physics.md]: https://github.com/peasanttide/peasanttide/blob/main/design/physics.md
pub fn kinematic_step(
    mesh: &NavMesh,
    cords: NavCords,
    duration: I16F16,
    tune: &Tune,
) -> Result<NavCords, NavError> {
    let mut state = cords.decode();
    let mut remaining = duration;
    let mut events = 0;

    while remaining.is_positive() && events < tune.max_events {
        events += 1;
        let tri = mesh.tri(state.tri).ok_or(NavError::UnknownTriangle {
            reference: state.tri,
            count: mesh.len(),
        })?;

        let taken = if let Some(event) = pick_next_event([
            calc_collision_vs_plane(tri, state, remaining, tune),
            calc_next_nav_tri(tri, state, remaining, tune),
        ]) {
            state = event.state;
            event.duration
        } else {
            state.position =
                NavTri::clamp_inside(state.position.add(state.velocity.mul(remaining)));
            remaining
        };

        remaining = remaining.saturating_sub(taken);
        state.velocity = apply_gravity(state.velocity, taken, tune);
        state.velocity = apply_drag(state.velocity, taken, tune);
    }

    Ok(NavCords::encode(state))
}

/// Gravity, which in a local frame is one subtraction.
///
/// The height axis *is* the up direction, so a downward acceleration touches
/// one component and leaves the two barycentric rates alone. That is the payoff
/// for measuring height along the geocentric up rather than along the face
/// normal, and it is why a million agents can be integrated without a rotation
/// each.
#[must_use]
#[inline]
pub fn apply_gravity(velocity: FinePoint, duration: I16F16, tune: &Tune) -> FinePoint {
    FinePoint::new(
        velocity.x(),
        velocity.y(),
        velocity
            .z()
            .saturating_sub(tune.gravity.saturating_mul(duration)),
    )
}

/// Drag, as a fraction of the speed shed per second.
///
/// First order in `duration` rather than exponential: the substeps are short,
/// an exponential would need a power, and what the number is for is keeping a
/// falling body from reaching a speed the encoding cannot hold.
#[must_use]
#[inline]
pub fn apply_drag(velocity: FinePoint, duration: I16F16, tune: &Tune) -> FinePoint {
    let shed = i64::from(tune.drag.to_bits()) * i64::from(duration.to_bits()) / 65_535;
    let one = i64::from(I16F16::ONE.to_bits());
    let kept = (one - shed).clamp(0, one);
    let scale = |value: I16F16| I16F16::from_bits((i64::from(value.to_bits()) * kept / one) as i32);
    FinePoint::new(
        scale(velocity.x()),
        scale(velocity.y()),
        scale(velocity.z()),
    )
}

/// The velocity a hit against the triangle's own plane leaves behind.
fn resolve_ground(tri: &NavTri, velocity: FinePoint, tune: &Tune) -> FinePoint {
    let world = tri.local_to_ecef().apply(velocity);
    let normal = tri.normal();
    let approach = along(world, normal);
    if !approach.is_negative() {
        return velocity;
    }
    let kept = if slides(approach, world, tune.slide_angle) {
        Factor16::MIN
    } else {
        tune.restitution
    };
    tri.ecef_to_local()
        .apply(reflect(world, normal, approach, kept))
}

/// The velocity a hit against an edge the body may not cross leaves behind.
///
/// The wall's normal is a row of the triangle's ECEF-to-local matrix, which
/// costs nothing to have: row 0 is orthogonal to both the second edge vector
/// and the up direction, so it is exactly the outward normal of a vertical wall
/// standing on the edge those two span. Edge 0's is the two rows summed,
/// because its constraint is the two coordinates summed.
fn resolve_wall(tri: &NavTri, velocity: FinePoint, edge: usize, tune: &Tune) -> FinePoint {
    let [first, second, _] = tri.ecef_to_local().rows();
    let inward = match edge {
        1 => first,
        2 => second,
        _ => first.add(second).neg(),
    };
    let Some(normal) = inward.normalize() else {
        return velocity;
    };

    let world = tri.local_to_ecef().apply(velocity);
    let approach = along(world, normal);
    if !approach.is_negative() {
        return velocity;
    }
    tri.ecef_to_local()
        .apply(reflect(world, normal, approach, tune.restitution))
}

/// `world` with its component along `normal` replaced by `-kept` of itself.
///
/// `kept` of zero is a slide and leaves the body moving along the surface;
/// anything more is a bounce.
#[inline]
fn reflect(world: FinePoint, normal: Direction, approach: I16F16, kept: Factor16) -> FinePoint {
    let damped = I16F16::from_bits(
        (i64::from(approach.to_bits()) * i64::from(kept.to_bits()) / 65_535) as i32,
    );
    let change = approach.saturating_add(damped).saturating_neg();
    world.add(fine_direction(normal).mul(change))
}

/// The component of a near-field vector along a unit direction.
#[inline]
fn along(vector: FinePoint, direction: Direction) -> I16F16 {
    let [x, y, z] = direction.to_array();
    let product = i128::from(vector.x().to_bits()) * i128::from(x.canonicalize().to_bits())
        + i128::from(vector.y().to_bits()) * i128::from(y.canonicalize().to_bits())
        + i128::from(vector.z().to_bits()) * i128::from(z.canonicalize().to_bits());
    let scale = i128::from(corvid_fixed::Signed32::MAX.to_bits());
    let rounded = if product >= 0 {
        (product + scale / 2) / scale
    } else {
        -((-product + scale / 2) / scale)
    };
    I16F16::saturating_from_bits(rounded as i64)
}

/// Whether a hit is shallow enough to slide.
///
/// `sin(incidence) = |approach| / |world|`, so the comparison is a pair of
/// squares and never an angle: no square root, no trigonometry, and the same
/// answer on every machine.
#[inline]
fn slides(approach: I16F16, world: FinePoint, limit: Angle16) -> bool {
    let sine = i128::from(limit.sin().to_bits());
    let normal = i128::from(approach.to_bits());
    let full = i128::from(corvid_fixed::Signed16::MAX.to_bits());
    normal * normal * full * full <= i128::from(world.length_squared()) * sine * sine
}
