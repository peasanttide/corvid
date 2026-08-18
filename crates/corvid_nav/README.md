# `corvid_nav`

The triangulated surface a world walks on, is indexed by, and diffuses across.

A level's ground is one [`NavMesh`]: a partition of the surface into triangles,
each carrying its three ECEF vertices, its three seams, and the two matrices
between its own local frame and the world. Beside them it carries a
[`NavGrid`], which answers "which triangle is this point on", and a
[`NavColours`], which says which triangles a thread may touch at the same time.
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
let stood = NavCords::centred(NavTriRef(0));
let after = kinematic_step(&mesh, stood, I16F16::from_f64(0.05), &Tune::default())?;
assert_eq!(after.tri, NavTriRef(0));
assert!(after.is_inside());

// The two halves of the square share an edge, so they never share a colour and
// a threaded tick takes two passes.
assert_eq!(mesh.colours().count(), 2);

// A rumour in one triangle is a rumour in both, and none of it is lost.
let mut heard = [I16F16::ONE, I16F16::ZERO];
diffuse_step(&mesh, &mut heard, Factor16::from_f64(0.5))?;
assert!(heard[1] > I16F16::ZERO);
assert_eq!(heard[0].saturating_add(heard[1]), I16F16::ONE);
# Ok(())
# }
```

## A position is not a point

It is a [`NavCords`]: a [`NavTriRef`], two barycentric coordinates, a height and
three velocities, sixteen bytes in all. Twelve of those are the part that
scales, because a crowd groups its agents by triangle and pays the reference
once per bucket; [`NavCords::local_bytes`] is that half on its own.

The coordinates are barycentric, so a [`NavCords`] is on the surface by
construction. "Is this agent standing on the ground" is not a question anybody
has to ask, because there is no way to write down an agent who is not.
[`NavCords::decode`] widens the codes into a [`NavState`] for the arithmetic and
[`NavCords::encode`] narrows them back, exactly, so a walk across a seam lands
where closed-form arithmetic says rather than a step downhill of it.

### What a coordinate is worth

**Sixteen bits span the triangle, so what a code is worth is a fact about the
triangle rather than about the crate.** [`NavTri::resolution`] is where a caller
asks, in metres, and a level of mixed triangles has a different answer on each
face:

| longest edge | one position code | one barycentric velocity code |
| --- | --- | --- |
| 2 m | 31 um | 0.12 mm/s |
| 8 m | 0.12 mm | 0.5 mm/s |
| 60 m | 0.92 mm | 3.7 mm/s |
| 600 m | 9.2 mm | 3.7 cm/s |
| 4096 m ([`MAX_EDGE`]) | 6.3 cm | 25 cm/s |

Height is the exception and is deliberately not relative: the height code spans
[`MAX_HEIGHT`] in metres, so it is 0.12 mm everywhere, as is the vertical
velocity's, which reaches +/-16 m/s at 0.5 mm/s a code.

This is what replaced an eight-metre edge limit. A real ground triangulated from
levelling points has triangles of very different sizes, and refining every one
of them to eight metres is what turned a district into a quarter of a million
triangles of identical shape. [`MAX_EDGE`] is four kilometres now and it is an
*arithmetic* limit -- a frame's columns are edge vectors in [`I16F16`], which
stops at 32.7 km, and the limit leaves a factor of eight of headroom for the
products a step forms. What used to be paid for in a constant is paid for in
[`Scaled3`] instead: an ECEF-to-local matrix has entries of about one over an
edge length, which on a large face would leave a handful of bits, so the entries
are scaled up to fill their width and the shift is carried beside them. A caller
multiplies as it always did and gets fifteen bits of the answer whatever size
the face is.

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
is a wall standing on that edge. That is why a walker does not fall off a cliff.

### Why a walker no longer sticks on a seam

Three rules, and each of them was a bug before it was a rule.

The wall an unwalkable edge stands for is **perpendicular to the face**, not to
the world. Its normal is a row of the triangle's [`NavTri::ecef_to_local`] --
free, because that row is already orthogonal to both the edge and the up
direction -- and that row is horizontal, which is the normal of an *upright*
wall. On a tilted face an upright wall's normal leans out of the face's plane,
so a bounce sent a walker into the ground, the ground collision slid it back
into the wall, and the two took turns. Taking the normal back into the plane
of the face first is one cross product at the moment of the hit and it makes a
body walking into a wall slide along it however steep the ground under it is.

Every event that resolves a boundary leaves the body [`EDGE_MARGIN`] **inside**
it, through [`NavTri::settle_inside`] rather than [`NavTri::clamp_inside`]. A
body left exactly on an edge is a body the next crossing test finds at distance
zero, so it crosses again, in no time, and the two triangles hand it back and
forth. The margin is two position codes: 61 um on a two-metre face, below
anything a caller can observe, and enough that the same boundary cannot answer
twice for nothing.

And two events in a row that take no time end the event loop for that tick. The
body spends what is left of it going where it is pointing. A third zero would
buy another zero and cost the rest of the budget, which is exactly how a peasant
used to stand still on an edge for good.

`tests/seam.rs` is the measurement: every state on every edge of both triangles
of a square tilted from flat to sixty degrees, every one of them walking, and
none of them allowed to cover less than a twentieth of the ground its own speed
says. Before these three rules, every slope from 45 degrees up had hundreds that
covered none of it, and they stayed stuck rather than recovering on the next
tick.

## The grid is a guess

[`NavGrid`] is a **sparse** index of the level's **tangent plane**, at a pitch
[`Tune::grid_pitch`] chooses and [`NavGrid::DEFAULT_PITCH`] -- 32 m -- by
default. Both halves of that are the design. A dense three-dimensional array
over an ECEF bounding box charges a city for the sky above it and for the
diagonal a level plane cuts through all three ECEF axes, and at Titonville that
put a ceiling on a district of 2,464 m on a side; [`NavPlane`] is the two
horizontal axes that ceiling goes away with.

Each [`NavCell`] holds every triangle whose corners' box covers it, in triangle
order, so a query has candidates to test rather than one guess to correct.
[`NavMesh::locate`] takes the first that actually contains the point and
[`NavMesh::walk_toward`] does the rest with the same crossing arithmetic a step
uses, so the answer never depends on the guess being right --
`tests/fold.rs` starts a walk from a deliberately wrong cell on a concave fold
and arrives anyway.

[`NavGrid::rebuild_cell`] is what an editor calls. Moving one building changes a
few triangles and leaves a quarter of a million alone, so one cell is re-cut
from what it held and what has arrived, and no other cell is touched.

## The colouring is what makes a tick threadable

[`NavMesh::colours`] gives a [`NavColours`]: a colour per triangle such that **no
two triangles that share an edge share a colour**, and the triangles of each
colour as a slice. A caller steps one class at a time and no two threads ever
touch adjacent triangles, which is what a crowd step and a diffusion step both
need, because a triangle's neighbours are exactly what they read and write.

The colouring is greedy in triangle index order -- triangle 0 takes colour 0,
and each triangle after it takes the lowest colour none of its already coloured
neighbours has. The order is stated because it has to be: two peers that
coloured differently would thread differently. A triangle has three edges so it
has at most three neighbours, so [`MAX_COLOURS`] is four and no surface needs
more. Ground triangulated in squares takes two, and a district's TIN takes three
or four.

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
the local frames, the grid over them, the colouring of them, the step that
advances one body, and the diffusion that spreads one number. It is `no_std`
with `alloc`, every operation is integer fixed point, and no floating point
appears anywhere in it -- a `from_f64` in a test or a `Default` is a
compile-time constant and never a tick.

It will not own the projection that turns a published coordinate reference
system into ECEF; that is `corvid_geo`'s, it needs transcendental arithmetic,
and it belongs at bake time with its result stored as fixed point. It will not
own pathfinding: a search over [`NavMesh::neighbours`] is a caller's, because
what a path costs is a question about the game and not about the ground. It will
not own the threads either -- [`NavColours`] says what may run at once and a
caller's own pool is what runs it. It will not own agents, steering, crowds or
what any of them believe -- the payload array is deliberately the caller's and
this crate never reads it. And it will not own rendering the surface; a
[`NavTri`] knows its vertices and `corvid_mesh` knows what a vertex buffer is.
