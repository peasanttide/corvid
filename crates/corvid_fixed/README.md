# corvid fixed math


## Key Types

I want
- I0F8, I24F8, I8F8
- Angle8, Angle16, Angle32
- Factor8, Factor16, Factor32
- Signed8, Signed16, Signed32

implement these fixed, factor, and angle types. they go from 0 to 1. make a macro and a struct to make it easier and compile fast. something like Fixed<Storage, Range, Wrap, Unit>

will also have to implement angle type (0..2PI with wraping) and snorm (-1..1) so put macro in fixed directory.

need support for nalgebra, mint, num_traits as well as arbitary and bytemuck pod.
(have feature flags for all of those)

the angle types should support trig functions.

all should be totally safe.

include extensive testing.

## Trig

trig functions only supported on the angle types.
and have various options that have various percesions.

## Tests
- converting to from f32 and f64
- wrap
- saturation
- overflow detaction
- num_traits
- sqrt
- isqrt
- round trip (should be lossless)
- all builtin rust f32/f64 functions should work

the functions must be const. if doing with f32 is faster and still const, that is fine. do not use f64 except for conversion.

## Const
all functions are const.
