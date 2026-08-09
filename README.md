# corvid

A deterministic multiplayer cross platform game framework.

Two machines running the same build over the same inputs produce the same
state, bit for bit, on every target Corvid supports. That one promise is
what the rest of this workspace is arranged around: the simulation is
integer arithmetic with no floating point anywhere in it, the state has one
encoding it is written down in and one digest it is checked with, and the
device boundary where floats are unavoidable is a named, separate layer.

Corvid is a facade crate, `corvid`, over a set of small `corvid_*` crates.
A game depends on the facade and gets the whole workspace; a tool that
wants only the wire format or only the hash depends on that crate alone and
builds nothing else.

## What is here today

The arithmetic is the foundation. `corvid_bits` answers the two integer
questions the rest of it keeps asking, how wide a magnitude is and how a
wide intermediate comes back to a component. `corvid_fixed` is the
fixed-point scalars built on that: the numbers, the normalized factors, and
the wrapping angles, with integer-only trigonometry that beats the
platform's own. `corvid_float` is the other side of the boundary, the `f32`
and `f64` utilities a device needs, every one of them `const`.

Geometry follows from the scalars. `corvid_vector` is the fixed-point
3-vectors, at the three widths a world position, an everyday offset and a
near-field offset each want. `corvid_rotation` is SO(3) at the same
precision: two packed codecs for the wire, an orthonormal basis for
rotating many points, and a unit quaternion for composing many rotations.
`corvid_transform` puts a rotation and a position together into the rigid
transform a game moves things with. `corvid_glm` is the float counterpart,
`nalgebra` in the column-major order a shader reads.

State is recorded and checked with `corvid_wire`, the one encoding a
snapshot is written down in, and `corvid_hash`, the one digest it is marked
with. `corvid_time` is simulation time: the integer tick, the tick rate,
and a fixed step that drops rather than banks. `corvid_files` is the trait
a level's files are read through, with a map in memory behind it.
`corvid_signal` carries state between threads as latest-value cells that
never wait for a consumer. `corvid_macros` holds the declarative macros the
other crates share.

## What is planned

The platform layer is the next thing to arrive, and it is where the crates
above meet a machine. An application crate owns the entry point and the
loop; input arrives cross platform through it, and rendering sets up `wgpu`
with the pass and phase scaffolding a game would otherwise write itself,
leaving the compute and the custom threads to the game. Shader compilation,
font rasterization and spatial audio sit alongside it, as does asset
management: reference-counted, cached, loaded asynchronously, with
placeholders and levels of detail.

Above the platform sit the pieces a game reasons in. Cameras are a position
and the intrinsics to drive one. Shapes are the primitives collision and
culling ask about, spheres and planes and cubes and axis-aligned boxes.
Color is representation, conversion and mixing. Behavior is the traits for
actions and state, where a player is a transform and an action together.
Networking is the interface the netcode plugs into, and replay reads a
recorded series of actions back for a demo.

Last is the command line tool, which builds and ships a game and checks
that it follows the layout and the conventions this workspace expects.

## Contributing

`CONTRIBUTING.md` is the contract a change to this workspace is held to,
and `ecosystem.md` tracks the crates outside it that Corvid speaks to.
`docs/determinism.md` is the argument behind the promise at the top of this
file.
