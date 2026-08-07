// The whole shader. The vertices arrive in clip space already, because a court
// seen from above is a handful of rectangles and transforming them on the CPU
// costs less than the uniform buffer a matrix would need.

struct Corner {
    @builtin(position) at: vec4<f32>,
    @location(0) tint: vec4<f32>,
}

@vertex
fn vertex_main(@location(0) at: vec2<f32>, @location(1) tint: vec4<f32>) -> Corner {
    var corner: Corner;
    corner.at = vec4<f32>(at, 0.0, 1.0);
    corner.tint = tint;
    return corner;
}

@fragment
fn fragment_main(corner: Corner) -> @location(0) vec4<f32> {
    return corner.tint;
}
