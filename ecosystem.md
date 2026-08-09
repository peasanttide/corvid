# Ecosystem roster

The crates outside this workspace that Corvid speaks to. An integration
named here sits behind a feature flag on the crate that offers it, off by
default, so a build that wants none of them pays for none of them.
`CONTRIBUTING.md` holds the rule; this file holds the roster.

## mint

The interchange vocabulary the graphics ecosystem shares: bare `Vector3`,
`Quaternion` and `RowMatrix3` structs with no maths on them, which every
library converts to and from rather than depending on each other. A game
with a glTF loader or a physics engine of its own speaks one of these at
that boundary, and the conversions are what let it hand the result straight
to Corvid.

## arbitrary

Structured input for fuzzing: a derive that builds a value of your type out
of a byte string, so a fuzz target asks for a rotation or a transform
rather than for bytes it has to validate itself. The bytes a fuzzer mutates
then map onto the shapes the code actually takes.

## bytemuck

Reinterpretation of a value as the bytes behind it, with the soundness
conditions checked by `Pod` and `Zeroable` rather than asserted. It is how
a transform or a camera matrix reaches a graphics device without an
`unsafe` block anywhere in this workspace.

## serde

Serialization: one derive, and any format that implements the data model
can read the result. It is the integration nearly every type here carries,
because a save file, a replay and a debug dump all reach for it, and a
state type that cannot be serialized is one a tool cannot read.
`corvid_wire` is built on it rather than beside it.

## wgpu

The graphics device: a portable implementation of WebGPU that compiles to
Vulkan, Metal, DirectX and the browser's own, so one renderer written
against it runs on all of them. It belongs to the binary rather than to the
simulation, which stays `no_std` on the other side of that line.

## jiff

Civil time: timestamps, zones and durations that a person reads. It labels
a replay file or a match record, and it is deliberately not simulation
time, which is `corvid_time`'s integer tick and nothing else.

## winit

The window and the event loop: creating a surface for `wgpu` to draw into,
and delivering the keyboard, pointer and gamepad events that drive a
simulation, with one interface over every platform's own.

## nalgebra

General linear algebra over floats. `corvid_glm` is the one crate that
names it, wrapping the `f32` types a shader reads; elsewhere it is an
optional conversion, so a caller can move between Corvid's deterministic
fixed-point types and the float ones a renderer wants.

## tracing

Instrumentation as structured spans and events rather than as printed
lines, with the subscriber that records them chosen by the binary. It is
the exception to the feature rule: `corvid_signal` takes it
unconditionally, because a publication that opened no span would be half a
handoff, and the span is part of what that crate is rather than something a
caller may decline.
