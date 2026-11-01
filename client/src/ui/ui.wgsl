// The client's own UI: textured quads in window pixels, drawn last, over the frame
// (m5-plan Decision 14).

struct Screen {
    // The window's width and height, in pixels.
    size: vec2<f32>,
}

@group(0) @binding(0) var<uniform> screen: Screen;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vertex_main(in: VertexInput) -> VertexOutput {
    // Window pixels run right and down from the top left. Clip space runs right and
    // up from the middle.
    let clip = in.position / screen.size * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);
    return VertexOutput(vec4<f32>(clip, 0.0, 1.0), in.uv, in.color);
}

// Blended in gamma, as the game does: the colours go out as they came in, to a view
// of the frame that doesn't convert them.
@fragment
fn fragment_gamma(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(atlas, atlas_sampler, in.uv) * in.color;
}

// For a frame that has no such view: linear light, for the sRGB format to convert
// back. Half-transparent pixels come out a little lighter than the game's.
@fragment
fn fragment_linear(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(atlas, atlas_sampler, in.uv) * in.color;
    return vec4<f32>(to_linear(color.rgb), color.a);
}

// The crosshair. The inverting blend reads the colour as how much to invert, so a
// transparent pixel has to be black, whatever colour it hides.
@fragment
fn fragment_invert(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(atlas, atlas_sampler, in.uv) * in.color;
    return vec4<f32>(color.rgb * color.a, color.a);
}

fn to_linear(color: vec3<f32>) -> vec3<f32> {
    let dark = color / 12.92;
    let bright = pow((color + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(bright, dark, color <= vec3<f32>(0.04045));
}
