// The terrain: block faces from the CPU mesher (`mesher.rs`), textured from
// one 2D texture array, tinted, alpha-tested or blended, shaded by face and
// corner, lit through the light map, and faded into the fog (`sky.rs`). The
// sky goes first, behind it all.
//
// Colour maths happens in linear light. The texture array is sampled through
// an sRGB view, so texels arrive linear; tints arrive sRGB-encoded and are
// converted here; the sRGB surface encodes the result on the way out.

struct Frame {
    // Rotation and projection only: positions arrive camera-relative.
    view_projection: mat4x4<f32>,
    // From the screen back to directions, for the sky.
    inverse_view_projection: mat4x4<f32>,
    // In linear light.
    sky_colour: vec4<f32>,
    fog_colour: vec4<f32>,
    // The glow towards a rising or setting sun, and how strong it is.
    glow: vec4<f32>,
    // Where the fog starts and ends, in blocks, and which way along x the
    // sun is.
    fog: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var block_textures: texture_2d_array<f32>;
@group(0) @binding(2) var block_sampler: sampler;
// Block levels across, sky levels down, in sRGB.
@group(0) @binding(3) var light_map: texture_2d<f32>;
@group(0) @binding(4) var light_sampler: sampler;

struct Vertex {
    // x, y and z in 1,024ths of a block within the sub-chunk, signed since a
    // model can reach out of its block, then the texture layer.
    @location(0) position_layer: vec4<i32>,
    // u and v in 128ths of the texture; the face (bits 0 to 2), the class
    // (bits 3 and 4) and how many blocks shade the corner (bits 5 and 6); and
    // the light, the sky's level high and the blocks' low.
    @location(1) uv_face: vec4<u32>,
    // The tint, sRGB-encoded, with the blend opacity in alpha.
    @location(2) tint: vec4<f32>,
    // Per draw: the sub-chunk's origin minus the camera, in blocks.
    @location(3) offset: vec4<f32>,
}

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    @location(2) @interpolate(flat) render_class: u32,
    @location(3) @interpolate(flat) shade: f32,
    @location(4) tint: vec4<f32>,
    // Where in the light map, between the corners' levels.
    @location(5) light: vec2<f32>,
    // The corner's shade, between the corners'.
    @location(6) occlusion: f32,
    // Where the point is from the eye, in blocks, for the fog.
    @location(7) relative: vec3<f32>,
}

// Render classes, as `mesher::Vertex` packs them.
const CUTOUT: u32 = 1u;
const OVERLAY: u32 = 2u;
const BLEND: u32 = 3u;

// Brightness by face, as Minecraft shades a face by the way it looks: sRGB
// factors of 0.5 below, 1 above, 0.8 north and south, 0.6 west and east, and 1
// for a plant's diagonal planes.
fn face_shade(face: u32) -> f32 {
    switch face {
        case 0u: { return 0.5; }
        case 2u, 3u: { return 0.8; }
        case 4u, 5u: { return 0.6; }
        default: { return 1.0; }
    }
}

// The sRGB transfer function, inverted.
fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

@vertex
fn vertex_main(v: Vertex) -> Varyings {
    var out: Varyings;
    let local = vec3<f32>(v.position_layer.xyz) / 1024.0;
    out.relative = local + v.offset.xyz;
    out.clip = frame.view_projection * vec4<f32>(out.relative, 1.0);
    out.uv = vec2<f32>(v.uv_face.xy) / 128.0;
    out.layer = u32(v.position_layer.w);
    out.render_class = (v.uv_face.z >> 3u) & 3u;
    out.shade = face_shade(v.uv_face.z & 7u);
    out.tint = vec4<f32>(srgb_to_linear(v.tint.rgb), v.tint.a);
    // A texel's middle for each level: block across, sky down.
    let light = v.uv_face.w;
    out.light = (vec2<f32>(f32(light & 15u), f32(light >> 4u)) + 0.5) / 16.0;
    // Each solid block around the corner takes a fifth.
    out.occlusion = 1.0 - 0.2 * f32((v.uv_face.z >> 5u) & 3u);
    return out;
}

// A crack multiplies what's under it, as Java's does: 50% grey leaves it as it is,
// darker darkens and lighter lightens. That's in encoded values, and these are in
// linear light, where encoded values to the power of 2.2 are 2^2.2 times the texel.
// The blend doubles what's returned, which lets it go over 1.
@fragment
fn fragment_crack(in: Varyings) -> @location(0) vec4<f32> {
    let texel = textureSample(block_textures, block_sampler, in.uv, in.layer);
    let factor = mix(vec3<f32>(1.0), texel.rgb * 4.595, texel.a);
    return vec4<f32>(factor * 0.5, 1.0);
}

@fragment
fn fragment_main(in: Varyings) -> @location(0) vec4<f32> {
    // Sampled first, while every invocation still runs: mipmapping needs
    // the neighbouring pixels' coordinates.
    let texel = textureSample(block_textures, block_sampler, in.uv, in.layer);
    let lit = textureSample(light_map, light_sampler, in.light).rgb;
    var color = texel.rgb * in.tint.rgb;
    var alpha = 1.0;
    switch in.render_class {
        case CUTOUT: {
            if texel.a < 0.5 {
                discard;
            }
        }
        case OVERLAY: {
            // Drawn opaque; alpha marks the texels that take the tint.
            color = texel.rgb * mix(vec3<f32>(1.0), in.tint.rgb, texel.a);
        }
        case BLEND: {
            alpha = texel.a * in.tint.a;
        }
        default: {}
    }
    // The face's shade, the corner's and the light multiply as Java multiplies
    // them, in sRGB, and only then become linear.
    let brightness = srgb_to_linear(lit * (in.shade * in.occlusion));
    // The fog: by the distance out, or up or down, whichever is more, as
    // Java's terrain fog is a cylinder round the eye.
    let distance = max(length(in.relative.xz), abs(in.relative.y));
    let fogged = clamp((distance - frame.fog.x) / max(frame.fog.y - frame.fog.x, 0.001), 0.0, 1.0);
    return vec4<f32>(mix(color * brightness, frame.fog_colour.rgb, fogged), alpha);
}

struct SkyVaryings {
    @builtin(position) clip: vec4<f32>,
    // Where on the screen, from -1 to 1 each way.
    @location(0) screen: vec2<f32>,
}

// The sky: one triangle over the whole screen, (-1, -1), (3, -1) and
// (-1, 3), drawn before the terrain.
@vertex
fn vertex_sky(@builtin(vertex_index) index: u32) -> SkyVaryings {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    var out: SkyVaryings;
    out.clip = vec4<f32>(corner, 1.0, 1.0);
    out.screen = corner;
    return out;
}

@fragment
fn fragment_sky(in: SkyVaryings) -> @location(0) vec4<f32> {
    let point = frame.inverse_view_projection * vec4<f32>(in.screen, 0.5, 1.0);
    let direction = normalize(point.xyz / point.w);
    // Java's sky is a plane 16 blocks up, fogged by how far away it is: the
    // fog's colour at the horizon and below, the sky's a little way up.
    let distance = 16.0 / max(direction.y, 0.0001);
    var colour = mix(frame.sky_colour.rgb, frame.fog_colour.rgb, clamp(distance / max(frame.fog.y, 1.0), 0.0, 1.0));
    // The glow round a rising or setting sun: Java's fan from the horizon
    // below it, fading out 50 degrees (0.87 radians) away.
    let from_sun = acos(clamp(dot(direction, vec3<f32>(frame.fog.z, 0.0, 0.0)), -1.0, 1.0));
    colour = mix(colour, frame.glow.rgb, frame.glow.a * max(1.0 - from_sun / 0.87, 0.0));
    return vec4<f32>(colour, 1.0);
}
