// One mip level of the tile atlas from the one above it, a 2x2 box at a time.
//
// The source is a view of one array layer at one mip level and the destination
// is a view of the same layer at the next, so the pass reads and writes
// different subresources of one texture, which is the one arrangement a device
// allows.
//
// There is no uniform and no vertex buffer. The vertex stage is a triangle
// covering the whole attachment and the fragment stage reads its own position,
// so which *tile* is being reduced is a scissor rectangle the caller sets and
// nothing this shader has to be told. That is what lets every tile of one layer
// share one pass.

@group(0) @binding(0) var source: texture_2d_array<f32>;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // A single triangle twice the size of the clip cube, which covers it with
    // no seam down the diagonal that two triangles would have.
    let x = f32(index / 2u) * 4.0 - 1.0;
    let y = f32(index & 1u) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    // Every level of the atlas is a whole number of tiles and every tile is a
    // power of two, so the 2x2 block below a destination texel is inside one
    // tile at every level down to a tile of a single texel. That is why the
    // box filter needs no clamp: it cannot reach a neighbour.
    let base = vec2<i32>(floor(position.xy)) * 2;
    let a = textureLoad(source, base, 0, 0);
    let b = textureLoad(source, base + vec2<i32>(1, 0), 0, 0);
    let c = textureLoad(source, base + vec2<i32>(0, 1), 0, 0);
    let d = textureLoad(source, base + vec2<i32>(1, 1), 0, 0);
    // Averaged in whatever space the load answered, which for an sRGB texture
    // is linear: a device decodes on load and encodes on store, so a plain mean
    // here is the linear-light box filter it looks like and not the gamma
    // mistake it would be on raw bytes.
    return (a + b + c + d) * 0.25;
}
