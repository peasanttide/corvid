// The shader `tests/offscreen.rs` draws with.
//
// It is a test fixture rather than part of the crate, and that is the point:
// after the pass graph went there is no shader in `corvid_render` at all, so a
// test that draws anything has to bring its own — exactly as a game does. What
// it exercises that a game's own shader would not is the two halves of the
// vertex format: a `Snorm16x4` position scaled by the mesh's own metre scale,
// and an octahedral normal decoded from the `Snorm8x2` pair `OctDirection`
// stores.

struct Uniforms {
    // Model, view and projection, already multiplied together on the CPU.
    clip: mat4x4<f32>,
    tint: vec4<f32>,
    // How many metres a full-deflection position component means, in `x`.
    scale: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
};

// The inverse of `OctDirection::encode`, in four lines and no unpacking: the
// attribute arrives as a `vec2<f32>` in [-1, 1] because `Snorm8x2` is what the
// type is laid out as.
fn decode_oct(e: vec2<f32>) -> vec3<f32> {
    var v = vec3<f32>(e.x, e.y, 1.0 - abs(e.x) - abs(e.y));
    if (v.z < 0.0) {
        // The lower hemisphere folded outward into the square's corners, so it
        // folds back: each component becomes the other's distance from the
        // diamond's edge, carrying its own sign, with zero counting as
        // positive exactly as the encoder has it.
        let signs = vec2<f32>(
            select(-1.0, 1.0, v.x >= 0.0),
            select(-1.0, 1.0, v.y >= 0.0),
        );
        v = vec3<f32>((1.0 - abs(vec2<f32>(v.y, v.x))) * signs, v.z);
    }
    return normalize(v);
}

@vertex
fn vertex_main(
    @location(0) position: vec4<f32>,
    @location(1) normal: vec2<f32>,
) -> VertexOut {
    var out: VertexOut;
    out.clip = u.clip * vec4<f32>(position.xyz * u.scale.x, 1.0);
    out.normal = decode_oct(normal);
    return out;
}

// The direction light travels: away from the viewer and downwards, already unit
// length. A face pointing back at a camera whose forward is +Y catches it, and
// the top of a box catches more of it than the front does.
const LIGHT: vec3<f32> = vec3<f32>(0.0, 0.6, -0.8);

// How much of the surface colour survives where the light does not reach.
const AMBIENT: f32 = 0.28;

@fragment
fn fragment_main(in: VertexOut) -> @location(0) vec4<f32> {
    let lambert = max(dot(normalize(in.normal), -LIGHT), 0.0);
    let shade = AMBIENT + (1.0 - AMBIENT) * lambert;
    return vec4<f32>(u.tint.rgb * shade, u.tint.a);
}
