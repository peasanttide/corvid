# Ecosystem roster

The crates outside this workspace that Corvid speaks to, and what each one
is for. An integration named here sits behind a feature flag on the crate
that offers it, off by default, so a build that wants none of them pays for
none of them. `CONTRIBUTING.md` holds the rule; this file holds the roster.

## Offered today

`serde` is serialization, and the one integration nearly every type carries:
a game's own save format, a replay, and a debug dump all reach for it, and a
state type that cannot be serialized is a state type a tool cannot read.
`corvid_wire` is the encoding a snapshot is written down in, and it is built
on `serde` rather than beside it.

`bytemuck` is the reinterpretation of a value as bytes, which is how a
transform or a camera matrix reaches a graphics device without an `unsafe`
block anywhere in this workspace. The impls come from `bytemuck`'s own
derive, or from `nalgebra` for the `f32` matrices in `corvid_glm`.

`mint` is the interchange vocabulary the graphics ecosystem shares. A game
that already has a glTF loader or a physics engine speaks one of `mint`'s
types at that boundary, and the conversions are what let it hand the result
straight to Corvid.

`nalgebra` is general linear algebra over floats. `corvid_glm` is the one
crate that names it, wrapping the `f32` types a shader reads; elsewhere it
is an optional conversion, so a caller can move between Corvid's
deterministic fixed-point types and the float ones a renderer wants.

`arbitrary` is structured input for fuzzing, so a fuzz target can ask for a
rotation or a transform rather than for bytes it has to validate itself.

`tracing` is instrumentation, and it is the exception to the feature rule:
`corvid_signal` takes it unconditionally, because a publication that opened
no span would be half a handoff, and the span is part of what that crate is
rather than something a caller may decline.

## Expected

`wgpu` is the graphics device, and `winit` the window and the input events
feeding it. Neither is a dependency yet; both arrive with the platform layer,
where they belong to the binary rather than to the simulation, and the
simulation stays `no_std` on the other side of that line.

`jiff` is civil time: the timestamps a replay file or a match record is
labelled with. It is deliberately not simulation time, which is
`corvid_time`'s integer tick and nothing else.
