// The whole of the fixture game's rendering: one triangle straight into clip
// space, in one colour.
//
// There is no camera, no uniform and no depth here on purpose. What
// `tests/windowless.rs` compares is a digest, so what this shader has to do is
// make the device rasterise something — anything — so the run it is comparing
// is a run in which a device did work.

@vertex
fn vertex_main(
    @location(0) position: vec4<f32>,
    @location(1) normal: vec2<f32>,
) -> @builtin(position) vec4<f32> {
    return vec4<f32>(position.xy, 0.5, 1.0);
}

@fragment
fn fragment_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.9, 0.3, 0.1, 1.0);
}
