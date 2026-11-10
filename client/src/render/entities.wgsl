// Entities until milestone 8 gives them models (m5-plan Decision 13): boxes in one colour
// per group, and dropped items as small spinning cards with their icon on. They're drawn
// in the terrain's pass, after its opaque faces, so the terrain hides them and they hide
// it, and they're lit and fogged as the terrain is (`terrain.wgsl`, which this mirrors).
//
// Positions arrive camera-relative, worked out on the CPU each frame: entities move
// every frame, and there are a few hundred vertices, not millions.

struct Frame {
    view_projection: mat4x4<f32>,
    inverse_view_projection: mat4x4<f32>,
    sky_colour: vec4<f32>,
    fog_colour: vec4<f32>,
    glow: vec4<f32>,
    // Where the fog starts and ends, in blocks, and which way along x the sun is.
    fog: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
// The terrain's texture array, through its sRGB view: texels arrive linear.
@group(0) @binding(1) var block_textures: texture_2d_array<f32>;
@group(0) @binding(2) var block_sampler: sampler;
// Block levels across, sky levels down, in sRGB.
@group(0) @binding(3) var light_map: texture_2d<f32>;
@group(0) @binding(4) var light_sampler: sampler;
// The UI's atlas, which holds the items' pictures. Not sRGB: texels arrive as in the file.
@group(0) @binding(5) var atlas: texture_2d<f32>;
@group(0) @binding(6) var atlas_sampler: sampler;

// Where a vertex's colour comes from: the tint alone; a block texture or an item picture
// cut out by alpha; a block texture whose alpha marks the texels the tint takes, drawn
// opaque, as the terrain draws a grass block's sides; or a block texture drawn whole, as
// the terrain draws an opaque block.
const FLAT: u32 = 0u;
const BLOCKS: u32 = 1u;
const ATLAS: u32 = 2u;
const OVERLAY: u32 = 3u;
const SOLID: u32 = 4u;

struct Vertex {
    // In blocks, from the eye.
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    // The texture array's layer.
    @location(2) layer: u32,
    // sRGB-encoded.
    @location(3) tint: vec4<f32>,
    // Where the colour comes from, the face (as the terrain's), and the light: the
    // sky's level high and the blocks' low.
    @location(4) info: vec4<u32>,
}

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    @location(2) @interpolate(flat) source: u32,
    @location(3) @interpolate(flat) shade: f32,
    @location(4) tint: vec4<f32>,
    @location(5) @interpolate(flat) light: vec2<f32>,
    @location(6) relative: vec3<f32>,
}

// As the terrain shades its faces: 0.5 below, 1 above, 0.8 north and south, 0.6 west
// and east.
fn face_shade(face: u32) -> f32 {
    switch face {
        case 0u: { return 0.5; }
        case 2u, 3u: { return 0.8; }
        case 4u, 5u: { return 0.6; }
        default: { return 1.0; }
    }
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

@vertex
fn vertex_main(v: Vertex) -> Varyings {
    var out: Varyings;
    out.relative = v.position;
    out.clip = frame.view_projection * vec4<f32>(v.position, 1.0);
    out.uv = v.uv;
    out.layer = v.layer;
    out.source = v.info.x;
    out.shade = face_shade(v.info.y);
    out.tint = v.tint;
    let light = v.info.z;
    out.light = (vec2<f32>(f32(light & 15u), f32(light >> 4u)) + 0.5) / 16.0;
    return out;
}

@fragment
fn fragment_main(in: Varyings) -> @location(0) vec4<f32> {
    // Both sampled every time, while every invocation still runs: mipmapping needs the
    // neighbouring pixels' coordinates.
    let block = textureSample(block_textures, block_sampler, in.uv, in.layer);
    let picture = textureSample(atlas, atlas_sampler, in.uv);
    let lit = textureSample(light_map, light_sampler, in.light).rgb;
    let tint = srgb_to_linear(in.tint.rgb);
    var color = tint;
    switch in.source {
        case BLOCKS, ATLAS: {
            var texel = block;
            if in.source == ATLAS {
                texel = vec4<f32>(srgb_to_linear(picture.rgb), picture.a);
            }
            // A picture's clear texels, cut out as the terrain cuts out leaves.
            if texel.a < 0.5 {
                discard;
            }
            color = texel.rgb * tint;
        }
        case OVERLAY: {
            color = block.rgb * mix(vec3<f32>(1.0), tint, block.a);
        }
        case SOLID: {
            color = block.rgb * tint;
        }
        default: {}
    }
    // Shade and light multiply in sRGB, as the terrain's do, and only then become linear.
    let brightness = srgb_to_linear(lit * in.shade);
    let distance = max(length(in.relative.xz), abs(in.relative.y));
    let fogged = clamp((distance - frame.fog.x) / max(frame.fog.y - frame.fog.x, 0.001), 0.0, 1.0);
    return vec4<f32>(mix(color * brightness, frame.fog_colour.rgb, fogged), 1.0);
}
