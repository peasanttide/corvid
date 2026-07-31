# corvid
Corvid Game Framework

## camera
a position and some intresnics and some utilities to help control cameras

## transform
fixed point transform
- vec3
- vec2
- angle
- fixed point rotation

## fixed
fixed point types.

## shapes

shapes and stuff

sphere
plane
cube
aabb

## color
color reresentation
conversion and mixing

## network
an interface for net code

## behavior
traits for actions, behaviors, state.

player is combo of transfrom and action.

## font
drawing text

## gui
bassicly egui

## time
time stamps simulation and stepping

## replay
replay a series of actions to help create demos

## application

the entrypoint

## input
cross platform 

## shader
compile shaders

## render

- has passes
- allows custome threads / compute
- basicly sets up wgpu for you and the rest is left as exersies to user
- does help with phases a little bit.

## audio

spatial audio

## cli

tool for initing, building and shipping corvid games.
ensures standards are being followed, crate layout, magic values are set, etc.
does cross platform builds.

## Asset

- memory management
- processing + cache
- async loading
- placeholders
- lods
- reference counted
