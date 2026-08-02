# determinism


## input

input pre-processing is floating point.

high resolution (64-bit rotation, `GlobalFinePoint` (`I48F16`) position ) transform for vr headset transform.

but it is all fixed point by time it gets to behavior.


## behavior

all behavior is entirely fixed point for determinism.


## time

uses jiff

## Render

Render engine uses gpu types so f32, i32, u32,. can also pack unorm8 snorm8 and fp16

Renderer does NOT need to be deterministic

