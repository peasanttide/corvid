// The two techniques a UI is: a signed-distance rounded box, and a glyph quad
// sampled out of a coverage atlas.
//
// One quad of four vertices per instance, drawn as a triangle strip, so there
// is no vertex buffer at all — the corner comes from the vertex index and
// everything else from the instance.

struct Viewport {
    // Physical pixels across and down, which is what a layout is in.
    size: vec2<f32>,
    // Sixteen bytes to the uniform, which is the minimum a binding is.
    pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> viewport: Viewport;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

// The corner of the unit quad this vertex is: (0,0), (1,0), (0,1), (1,1).
fn corner(index: u32) -> vec2<f32> {
    return vec2<f32>(f32(index & 1u), f32(index >> 1u));
}

// Layout space is pixels right and down from the top left; clip space is
// [-1, 1] right and up from the middle.
fn to_clip(pixels: vec2<f32>) -> vec4<f32> {
    let ndc = vec2<f32>(
        pixels.x / viewport.size.x * 2.0 - 1.0,
        1.0 - pixels.y / viewport.size.y * 2.0,
    );
    return vec4<f32>(ndc, 0.0, 1.0);
}

struct RectInstance {
    @location(0) rect: vec4<f32>,
    @location(1) fill: vec4<f32>,
    @location(2) border: vec4<f32>,
    // Border width, corner radius, and two the shader does not read.
    @location(3) params: vec4<f32>,
};

struct RectVertex {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) half_size: vec2<f32>,
    @location(2) fill: vec4<f32>,
    @location(3) border: vec4<f32>,
    @location(4) params: vec4<f32>,
};

@vertex
fn rect_vertex(@builtin(vertex_index) index: u32, instance: RectInstance) -> RectVertex {
    let unit = corner(index);
    var out: RectVertex;
    out.clip = to_clip(instance.rect.xy + unit * instance.rect.zw);
    out.half_size = instance.rect.zw * 0.5;
    out.local = (unit - vec2<f32>(0.5)) * instance.rect.zw;
    out.fill = instance.fill;
    out.border = instance.border;
    out.params = instance.params;
    return out;
}

// The signed distance to a rounded box: negative inside, positive outside, and
// in pixels either way, which is what makes one pixel of antialiasing a
// subtraction rather than a screen-space derivative.
fn rounded_box(local: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let inner = half_size - vec2<f32>(radius);
    let q = abs(local) - inner;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

@fragment
fn rect_fragment(in: RectVertex) -> @location(0) vec4<f32> {
    let radius = clamp(in.params.y, 0.0, min(in.half_size.x, in.half_size.y));
    let distance = rounded_box(in.local, in.half_size, radius);
    let coverage = clamp(0.5 - distance, 0.0, 1.0);
    var colour = in.fill;
    if in.params.x > 0.0 {
        // Inside the border is the fill; the ring between the edge and the
        // border width is the border colour.
        let inside = clamp(0.5 - (distance + in.params.x), 0.0, 1.0);
        colour = mix(in.border, in.fill, inside);
    }
    return vec4<f32>(colour.rgb, colour.a * coverage);
}

struct GlyphInstance {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) tint: vec4<f32>,
};

struct GlyphVertex {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
};

@vertex
fn glyph_vertex(@builtin(vertex_index) index: u32, instance: GlyphInstance) -> GlyphVertex {
    let unit = corner(index);
    var out: GlyphVertex;
    out.clip = to_clip(instance.rect.xy + unit * instance.rect.zw);
    out.uv = mix(instance.uv.xy, instance.uv.zw, unit);
    out.tint = instance.tint;
    return out;
}

@fragment
fn glyph_fragment(in: GlyphVertex) -> @location(0) vec4<f32> {
    let coverage = textureSample(atlas, atlas_sampler, in.uv).r;
    return vec4<f32>(in.tint.rgb, in.tint.a * coverage);
}
