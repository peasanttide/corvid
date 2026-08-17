# `corvid_geo`

The ground a world-scale game stands on: the coordinate reference systems
archives publish in, turned into the one the simulation works in.

```rust
use corvid_fixed::{Angle32, I24F8, Pitch32};
use corvid_geo::{Anchor, Ellipsoid, Geodetic, Polygon, Ring, ground};
use corvid_vector::globalpoint;

// A level says where it is once, in a latitude and a longitude somebody can
// look up. La Folie Titon, on the faubourg Saint-Antoine.
let anchor = Anchor::new(
    Geodetic::new(
        Pitch32::from_degrees(48.8524),
        Angle32::from_degrees(2.3855),
        I24F8::ZERO,
    ),
    Ellipsoid::WGS84,
);

// After that its coordinates are metres east, north and up from that spot.
let gate = globalpoint(40, -12, 0);
let world = anchor.to_ecef(gate).expect("the faubourg is on the earth");
assert_eq!(anchor.to_local(world), Some(gate));

// And the shapes standing on it are polygons in those same metres.
let yard = Polygon::new(
    Ring::new(vec![ground(0, 0), ground(60, 0), ground(60, 40), ground(0, 40)]),
    vec![Ring::new(vec![
        ground(20, 10), ground(40, 10), ground(40, 30), ground(20, 30),
    ])],
);
assert_eq!(yard.triangulate().map(|cut| cut.area()), Ok(yard.signed_area()));
```

## Two halves, and the split is the point

The half that is always compiled is `no_std`, integer-only, and safe to call
from a tick. [`Geodetic`] holds a latitude, a longitude and a height in
`corvid_fixed`'s own types; [`Geodetic::to_ecef`] and [`Geodetic::from_ecef`]
move between it and the earth-centred, earth-fixed metres a
`corvid_vector::GlobalPoint` carries; and [`Anchor`] is the local east-north-up
frame that makes a level's coordinates read as a tape measure read them. No
floating point appears anywhere along that path, so two machines that convert
the same position get the same bit pattern and a hash over it means something.

The other half is behind the `project` feature, is off by default, and pulls
`std`. [`Wgs84`] is a position in degrees, [`ConformalConic`] is Lambert
Conformal Conic with two standard parallels, and
[`ConformalConic::LAMBERT93`] is EPSG:2154 with the parameters the registry
states -- GRS80, standard parallels at 44 and 49 north, latitude of origin
46.5 north, central meridian 3 east, false easting 700000, false northing
6600000. **Everything behind that feature runs at bake time and nothing behind
it may run in a tick.** A conformal conic is a logarithm and a power and a
geodetic inverse in closed form is a cube root; there is no integer version of
those worth having, so they are computed once, when a level is built, and what
gets stored is the fixed-point [`Geodetic`] that came out. [`Wgs84::to_geodetic`]
is that seam, and it is the only door between the halves.

## What the numbers can say

| | Type | Resolution |
|---|---|---|
| Latitude | `corvid_fixed::Pitch32`, clamping at the poles | 9.3 mm of northing |
| Longitude | `corvid_fixed::Angle32`, wrapping at the antimeridian | 9.3 mm of easting at the equator |
| Height | `corvid_fixed::I24F8` | 3.9 mm |
| ECEF | `corvid_vector::GlobalPoint` | 3.9 mm |

A pitch and an angle are the right types for a latitude and a longitude
because their *semantics* are already right: a latitude clamps and a longitude
wraps, so no value of either is invalid and neither conversion needs a check
it could get wrong. What they cost is 9.3 mm, and that is the floor under
everything here.

The integer conversions land within a centimetre of the same formulae
evaluated in `f64`, and `tests/geodesy.rs` asserts it on Paris and on its four
corners. That centimetre is not slop in the arithmetic; it is three
millimetres per sine, because a `corvid_fixed::Signed32` sine carries `4.7e-10`
and the earth's radius is `6.4e6` metres, and two of them multiply together
before the result is rounded into a 3.9 mm grid. Everything narrower than that
is exact: `e^2` is carried at `2^-48`, the square root of `1 - e^2 sin^2` is
taken by `isqrt` on a widened Q96 intermediate, and every division rounds once.

[`Geodetic::from_ecef`] answers Bowring's method, which is closed form and
needs no iteration. Its error is under a micron of latitude anywhere near the
surface, which is four orders below what a `Pitch32` can express, so iterating
would refine a number with nowhere to put the refinement.

## Polygons, because a map is made of them

[`Polygon`] is an outer [`Ring`] and the holes punched in it, drawn from
[`GroundPoint`]s -- metres east and north on an [`Anchor`]'s tangent plane. A
ring answers its [`Ring::winding`], its [`Ring::signed_area`] and whether it
[`Ring::contains`] a point by winding number, and construction reorients
whatever an archive handed over so that outer rings run counterclockwise and
holes clockwise.

[`Polygon::triangulate`] is ear clipping with the holes bridged in first, and
it is integer-only for the reason everything else here is: the sign of a cross
product decided in an `i128` is the true sign, so the same polygon cuts into
the same triangles in the same order on every machine. `corvid_nav` indexes
those triangles, which is why that is a requirement rather than a nicety.
Every triangle comes out counterclockwise, none is degenerate, and
[`Triangulation::area`] equals [`Polygon::signed_area`] exactly -- the
triangles index the polygon's own points, because bridging duplicates a vertex
of the boundary rather than a point of the shape.

## Scope

Coordinate reference systems, the frames between them, and the flat geometry a
map is drawn in. Geodetic and ECEF and a local tangent frame; one map
projection family, with the French grid as a constant; polygons with holes,
and a triangulation of one.

Not a geoid model, so a height here is above the ellipsoid and not above the
sea. Not a datum shift, because RGF93 and WGS84 agree far inside what these
types can express and a pair that genuinely differs needs seven parameters
this crate does not carry. Not a file format: reading a shapefile is
`corvid_files` and a level's data packs are `corvid_asset`. Not a navmesh --
`corvid_nav` consumes the triangulation and owns everything after it -- and
not a straight skeleton, because a roof is a building's problem and a building
is a game's.
