# `corvid_nav`

The triangulated surface a world walks on, is indexed by, and diffuses across.

A level's ground is one [`NavMesh`]: a partition of the surface into triangles
of at most [`MAX_EDGE`] on a side, each carrying its three ECEF vertices, its
three seams, and the two matrices between its own local frame and the world.
That one structure is the navmesh, the spatial index, the cold tier's storage
and the medium a rumour travels through, and it is one structure because a
million agents cannot afford four.

```rust
use corvid_fixed::{Factor16, I16F16, I24F8};
use corvid_nav::{NavCords, NavMesh, NavTriRef, Tune, diffuse_step, kinematic_step};
use corvid_vector::GlobalPoint;
# fn metres(x: f64, y: f64, z: f64) -> GlobalPoint {
#     GlobalPoint::new(I24F8::from_f64(x), I24F8::from_f64(y), I24F8::from_f64(z))
# }
# fn main() -> Result<(), corvid_nav::NavError> {
// A four-metre patch of ground, on the earth's surface where a level is.
let radius = 6_371_000.0;
let vertices = [
    metres(0.0, 0.0, radius),
    metres(4.0, 0.0, radius),
    metres(0.0, 4.0, radius),
    metres(4.0, 4.0, radius),
];
let mesh = NavMesh::new(&vertices, &[[0, 1, 2], [3, 2, 1]], &Tune::default())?;
assert_eq!(mesh.neighbours(NavTriRef(0)).count(), 1);

// A body standing in the middle of one triangle stays on it for a tick.
let stood = NavCords::at(NavTriRef(0), [85, 85, 0]);
let after = kinematic_step(&mesh, stood, I16F16::from_f64(0.05), &Tune::default())?;
assert_eq!(after.tri, NavTriRef(0));
assert!(after.is_inside());

// A rumour in one triangle is a rumour in both, and none of it is lost.
let mut heard = [I16F16::ONE, I16F16::ZERO];
diffuse_step(&mesh, &mut heard, Factor16::from_f64(0.5))?;
assert!(heard[1] > I16F16::ZERO);
assert_eq!(heard[0].saturating_add(heard[1]), I16F16::ONE);
# Ok(())
# }
```

## A position is not a point

It is a [`NavCords`]: a [`NavTriRef`], two barycentric bytes, a height byte and
three velocity bytes. Six of those ten are the part that scales, because a crowd
groups its agents by triangle and pays the reference once per bucket;
[`NavCords::local_bytes`] is that half on its own. Eight bits across a triangle
of at most eight metres is about three centimetres, which is deliberately coarse
and is what a million of them fitting in cache costs.

The coordinates are barycentric, so a [`NavCords`] is on the surface by
construction. "Is this agent standing on the ground" is not a question anybody
has to ask, because there is no way to write down an agent who is not.
[`NavCords::decode`] widens the bytes into a [`NavState`] for the arithmetic and
[`NavCords::encode`] narrows them back, exactly, so a walk across a seam lands
where closed-form arithmetic says rather than a step downhill of it.

## A triangle is a frame

Each [`NavTri`] holds the affine combination [crs.md] specifies: local `x` and
`y` weight the first two vertices, the third weight is what is left, and local
`z` is metres of height along the geocentric up, which is
[`NavTri::down`] reversed. Two consequences run through everything else. The
plane of the triangle is `z == 0`, so a ground collision is a sign test on one
number. And gravity is `-Z` in every triangle's frame, so integrating a
ballistic substep is one subtraction and never a rotation.

The price is that a face whose plane contains the up axis -- a wall -- has no
frame at all: its local-to-ECEF matrix is singular, its determinant being
`2 * area * cos(slope)`. [`NavMesh::new`] refuses such a face with
[`NavError::FaceTooSteep`], which is a real constraint on what a TIN may
contain and not a tolerance anybody can widen.

A [`NavTriEdge`] holds the neighbour and the map into its frame, so crossing a
seam is a multiply rather than a search: a position goes through
[`NavTriEdge::local_to_next`] and a velocity through [`NavTriEdge::vel_to_next`],
which is the same matrix without the translation. Edge `i` is between vertex `i`
and vertex `i + 1 mod 3`. Whether the seam may be crossed on foot is
precomputed into [`NavTriEdge::is_walkable`] and derivable at any time from
[`NavMesh::derive_walkable`], which recomputes the seam's map from the two
frames and asks the two questions the flag answers: do the heights agree where
the two triangles meet, as [`NavMesh::heights_agree`] asks on its own, and is
the far face shallower than [`Tune::max_slope`]. `tests/walkable.rs` holds the
stored answer to the derived one on every seam of every fixture.

There is only ever one triangle for a given coordinate. Where a query lands on a
shared edge the lower edge index wins and where it lands on a shared vertex the
lower triangle index does, which is a rule about determinism rather than about
geometry: two peers asking the same question have to get the same answer without
agreeing on anything but the mesh.

## A step is a loop over events

[`kinematic_step`] is [physics.md]'s loop and the order of it is the design.
While time remains it computes [`calc_collision_vs_plane`] and
[`calc_next_nav_tri`] against the straight line the body is on, takes whichever
[`NavEvent`] [`pick_next_event`] says happens first, advances exactly to it, and
only then applies [`apply_gravity`] and [`apply_drag`] over the time that took.
Forces are integrated between events rather than through them, so a bounce is
resolved against the velocity the body arrived with. [`Tune::max_events`] caps
the iterations; a walking agent uses two or three.

[`calc_collision_vs_plane`] compares the angle of incidence with
[`Tune::slide_angle`] as a pair of squared sines, so no trigonometry runs in a
tick. Under the threshold the velocity is projected onto the face and the body
slides, which is what makes a ramp accelerate somebody downhill: gravity goes
into the plane, the projection removes the part that would go through it, and
what is left points down the slope. Over the threshold the normal component
reverses and keeps [`Tune::restitution`] of itself.

[`calc_next_nav_tri`] treats the three edges as three linear inequalities, so
the crossing time is a division and the crossing point is on the line by
construction. A walkable seam carries the position and the velocity across and
clamps the arrival above the neighbour's own plane, so a body never ends a step
underground. An unwalkable seam, and a boundary edge with no neighbour at all,
is a vertical wall standing on that edge, and its normal is a row of the
triangle's [`NavTri::ecef_to_local`] -- free, because that row is already
orthogonal to both the edge and the up direction. That is why a walker does not
fall off a cliff.

## The grid is a guess

[`NavGrid`] divides the level into cells of [`NavGrid::CELL`] and stores in each
the triangle covering most of it, measured by where a triangle's vertices, edge
midpoints and centroid land. [`NavMesh::locate`] starts from that cell and
[`NavMesh::walk_toward`] does the rest with the same crossing arithmetic a step
uses, so the answer never depends on the guess being right --
`tests/fold.rs` starts a walk from a deliberately wrong cell on a concave fold
and arrives anyway.

## The payload is the caller's

This crate does not know what a peasant is, so per-triangle payload is an
index-parallel array the caller owns rather than a generic parameter.
[`NavMesh::tris`] hands out a slice whose indices are [`NavTriRef`] values; keep
a vector of the same length beside it and index both with the same number. A
generic would have put the payload's type into the signature of every function
here, which is a large price for a `Vec` the caller can write themselves.

[`diffuse_step`] is what the crowd is built on. It spreads a per-triangle
[`I16F16`] field one step across the seams, written as flows on edges rather
than as an average of neighbours, so what it takes from one triangle it gives to
the other in the same integer and the total is conserved exactly.
[`NavMesh::neighbours`] iterates the seams in edge order, which is the order
everything here iterates in, because a simulation that walked a hash map would
desync the moment two peers allocated differently.

[`I16F16`]: corvid_fixed::I16F16
[crs.md]: https://github.com/peasanttide/peasanttide/blob/main/design/crs.md
[physics.md]: https://github.com/peasanttide/peasanttide/blob/main/design/physics.md

## Scope

This crate owns the surface and what moves along it: the triangles, the seams,
the local frames, the grid over them, the step that advances one body, and the
diffusion that spreads one number. It is `no_std` with `alloc`, every operation
is integer fixed point, and no floating point appears anywhere in it -- a
`from_f64` in a test or a `Default` is a compile-time constant and never a tick.

It will not own the projection that turns a published coordinate reference
system into ECEF; that is `corvid_geo`'s, it needs transcendental arithmetic,
and it belongs at bake time with its result stored as fixed point. It will not
own pathfinding: a search over [`NavMesh::neighbours`] is a caller's, because
what a path costs is a question about the game and not about the ground. It will
not own agents, steering, crowds or what any of them believe -- the payload
array is deliberately the caller's and this crate never reads it. And it will
not own rendering the surface; a [`NavTri`] knows its vertices and
`corvid_mesh` knows what a vertex buffer is.
